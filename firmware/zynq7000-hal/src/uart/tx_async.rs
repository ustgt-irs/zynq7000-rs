//! Asynchronous UART transmitter (TX) implementation.
use core::{
    convert::Infallible,
    marker::PhantomData,
    sync::atomic::{AtomicBool, AtomicPtr, AtomicUsize, Ordering},
};

use arbitrary_int::u6;
use embassy_sync::waitqueue::AtomicWaker;
use zynq7000::uart::FifoTrigger;

use crate::uart::{FIFO_DEPTH, Tx, UartId};

static UART_TX_WAKERS: [AtomicWaker; 2] = [const { AtomicWaker::new() }; 2];
static TX_CONTEXTS: [TxContext; 2] = [const { TxContext::new() }; 2];
// Completion flag. Kept outside of the context structure as an atomic to avoid
// critical section.
static TX_DONE: [AtomicBool; 2] = [const { AtomicBool::new(false) }; 2];

/// TX context structure. Plain atomics rather than a `critical_section::Mutex<RefCell<_>>` so it
/// can live in a `static` array directly. `raw_data` doubles as the "transfer active" flag: it
/// is always published last (`Release`) after `transfer_len`/`progress`, and read first
/// (`Acquire`) before them, so a reader that observes it non-null is guaranteed to see the
/// matching, not stale, `transfer_len`/`progress`.
struct TxContext {
    progress: AtomicUsize,
    raw_data: AtomicPtr<u8>,
    transfer_len: AtomicUsize,
}

impl TxContext {
    const fn new() -> Self {
        Self {
            progress: AtomicUsize::new(0),
            raw_data: AtomicPtr::new(core::ptr::null_mut()),
            transfer_len: AtomicUsize::new(0),
        }
    }
}

/// This is a generic interrupt handler to handle asynchronous UART TX operations for a given
/// UART peripheral.
///
/// # Safety
///
/// The user has to call this once in the interrupt handler responsible for the TX interrupts on
/// the given UART bank.
pub unsafe fn on_interrupt_tx(peripheral: UartId) {
    // SAFETY: The interrupt handler only accesses the TX interrupt registers.
    let mut tx_with_irq = unsafe { Tx::steal(peripheral) };
    let idx = peripheral as usize;
    let enabled_irqs = tx_with_irq.regs().read_enabled_interrupts();
    // IRQ is not related to TX.
    if !enabled_irqs.tx_over()
        && !enabled_irqs.tx_near_full()
        && !enabled_irqs.tx_full()
        && !enabled_irqs.tx_empty()
    {
        return;
    }

    let interrupt_status = tx_with_irq.regs().read_interrupt_status();
    // Disable interrupts, re-enable them later.
    tx_with_irq.disable_interrupts();
    // Clear interrupts.
    tx_with_irq.clear_interrupts();

    let context = &TX_CONTEXTS[idx];
    // `Acquire` pairs with the `Release` stores in `start_transfer`, `poll_transfer_done` and the
    // guard's `Drop`: seeing a non-null pointer here guarantees `transfer_len`/`progress` below
    // are the values published together with it, not stale ones from a previous transfer.
    let raw_data_ptr = context.raw_data.load(Ordering::Acquire);
    // No transfer active.
    if raw_data_ptr.is_null() {
        return;
    }
    let slice_len = context.transfer_len.load(Ordering::Relaxed);
    let mut progress = context.progress.load(Ordering::Relaxed);
    // SAFETY: The transfer guard borrows the slice and clears the pointer before the borrow
    // ends.
    let slice = unsafe { core::slice::from_raw_parts(raw_data_ptr as *const u8, slice_len) };

    if (progress >= slice_len && interrupt_status.tx_empty()) || slice_len == 0 {
        // Transfer is done. `Release` publishes the final `progress` value to whichever context
        // observes `TX_DONE` via the `Acquire` swap in `poll_transfer_done`.
        TX_DONE[idx].store(true, Ordering::Release);
        tx_with_irq.disable_interrupts();
        tx_with_irq.clear_interrupts();
        UART_TX_WAKERS[idx].wake();
        return;
    }

    // Pump the FIFO.
    while progress < slice_len {
        match tx_with_irq.write_fifo(slice[progress]) {
            Ok(_) => progress += 1,
            Err(nb::Error::WouldBlock) => break,
        }
    }
    let remaining = slice_len - progress;
    if remaining > FIFO_DEPTH {
        tx_with_irq.regs.write_tx_fifo_trigger(
            FifoTrigger::builder()
                .with_trigger(u6::new((FIFO_DEPTH / 2) as u8))
                .build(),
        );
    }
    context.progress.store(progress, Ordering::Relaxed);

    tx_with_irq.enable_interrupts(remaining > FIFO_DEPTH);
}

/// Returns the number of written bytes once the interrupt handler signals completion.
fn poll_transfer_done(id: UartId, cx: &mut core::task::Context<'_>) -> core::task::Poll<usize> {
    let idx = id as usize;
    UART_TX_WAKERS[idx].register(cx.waker());
    // `Acquire` pairs with the `Release` store in `on_interrupt_tx`.
    if !TX_DONE[idx].swap(false, Ordering::Acquire) {
        return core::task::Poll::Pending;
    }
    let context = &TX_CONTEXTS[idx];
    // `Release` pairs with the `Acquire` load in `on_interrupt_tx`, so a spurious interrupt for
    // this slot after completion can never see a dangling pointer.
    context
        .raw_data
        .store(core::ptr::null_mut(), Ordering::Release);
    core::task::Poll::Ready(context.progress.load(Ordering::Relaxed))
}

/// Stops the transfer on completion and on cancellation, because the interrupt handler reads
/// from the buffer until then.
struct TransferGuard<'tx, 'buf>(&'tx mut Tx, PhantomData<&'buf [u8]>);

impl Drop for TransferGuard<'_, '_> {
    fn drop(&mut self) {
        self.0.disable_interrupts();
        // Clear the buffer pointer so a spurious or future interrupt for this UART can never
        // dereference it. After completion, this was already done and is harmless.
        let context = &TX_CONTEXTS[self.0.uart_id() as usize];
        context.progress.store(0, Ordering::Relaxed);
        context
            .raw_data
            .store(core::ptr::null_mut(), Ordering::Release);
    }
}

/// Asynchronous UART transmitter (TX) driver.
pub struct TxAsync {
    tx: Tx,
}

impl TxAsync {
    /// Constructor.
    ///
    /// The second argument specifies whether the [on_interrupt_tx] function will be registered
    /// in the HAL interrupt map. You might need to skip this in case you have your own
    /// interrupt handler which also handles RX interrupts.
    pub fn new(tx: Tx, register_interrupt_handler: bool) -> Self {
        if register_interrupt_handler {
            match tx.uart_id() {
                UartId::Uart0 => {
                    unsafe fn uart0_interrupt_handler() {
                        // SAFETY: Only called from the UART 0 interrupt.
                        unsafe {
                            on_interrupt_tx(UartId::Uart0);
                        }
                    }
                    crate::register_interrupt(
                        crate::gic::Interrupt::Spi(crate::gic::SpiInterrupt::Uart0),
                        uart0_interrupt_handler,
                    )
                }
                UartId::Uart1 => {
                    unsafe fn uart1_interrupt_handler() {
                        // SAFETY: Only called from the UART 1 interrupt.
                        unsafe {
                            on_interrupt_tx(UartId::Uart1);
                        }
                    }
                    crate::register_interrupt(
                        crate::gic::Interrupt::Spi(crate::gic::SpiInterrupt::Uart1),
                        uart1_interrupt_handler,
                    )
                }
            }
        }

        Self { tx }
    }

    /// Write a buffer asynchronously and return the number of written bytes.
    ///
    /// The transfer starts on the first poll. Dropping the future stops the transfer, but part of
    /// the buffer might already have been written.
    ///
    /// Leaking the future after the first poll, for example with [core::mem::forget], is not
    /// supported. The interrupt handler would keep reading from the buffer after the borrow ended.
    pub async fn write(&mut self, buf: &[u8]) -> usize {
        if buf.is_empty() {
            return 0;
        }
        let id = self.tx.uart_id();
        let _guard = self.start_transfer(buf);
        core::future::poll_fn(|cx| poll_transfer_done(id, cx)).await
    }

    fn start_transfer<'tx, 'buf>(&'tx mut self, data: &'buf [u8]) -> TransferGuard<'tx, 'buf> {
        let tx = &mut self.tx;
        let idx = tx.uart_id() as usize;
        TX_DONE[idx].store(false, Ordering::Relaxed);
        tx.disable_interrupts();
        tx.disable();

        let init_fill_count = core::cmp::min(data.len(), FIFO_DEPTH);
        let context = &TX_CONTEXTS[idx];
        // Publish the guarded fields before opening the gate (`raw_data`) with `Release`, so a
        // reader that observes `raw_data` non-null via the `Acquire` load in `on_interrupt_tx`
        // is guaranteed to see these too, rather than stale values from a previous transfer.
        context.transfer_len.store(data.len(), Ordering::Relaxed);
        context.progress.store(init_fill_count, Ordering::Relaxed);
        context
            .raw_data
            .store(data.as_ptr() as *mut u8, Ordering::Release);
        // Apparently, we need to enable the UART before we are able to write something into
        // the FIFO.
        tx.enable(false);
        if data.len() > FIFO_DEPTH {
            tx.regs.write_tx_fifo_trigger(
                FifoTrigger::builder()
                    .with_trigger(u6::new((FIFO_DEPTH / 2) as u8))
                    .build(),
            );
        }
        for data in data.iter().take(init_fill_count) {
            tx.write_fifo_unchecked(*data);
        }
        tx.enable_interrupts(data.len() > FIFO_DEPTH);
        TransferGuard(tx, PhantomData)
    }

    /// Release the underlying blocking TX driver.
    pub fn release(self) -> Tx {
        self.tx
    }
}

impl embedded_io::ErrorType for TxAsync {
    type Error = Infallible;
}

impl embedded_io_async::Write for TxAsync {
    /// Write a buffer asynchronously.
    ///
    /// This implementation is not side effect free, and a started future might have already
    /// written part of the passed buffer.
    async fn write(&mut self, buf: &[u8]) -> Result<usize, Self::Error> {
        Ok(self.write(buf).await)
    }

    /// This implementation does not do anything.
    async fn flush(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}
