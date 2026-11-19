#![no_std]

use core::{cell::RefCell, panic::PanicInfo};

use aarch32_cpu::asm::nop;
use critical_section::Mutex;
use z7_defmt_rtt as _;
use zynq7000_hal::time::Hertz;

/// Clock was already initialized by PS7 Init TCL script or FSBL, we just read it.
pub const PS_CLOCK_FREQUENCY: Hertz = Hertz::from_raw(33_333_333);

/// One-shot cross-core hand-off cell.
///
/// CPU0 constructs a value and stores it with [Self::put] before releasing CPU1 via
/// `zynq7000_rt::smp::start_core1`. CPU1 retrieves it exactly once with [Self::take].
pub struct Handoff<T>(Mutex<RefCell<Option<T>>>);

impl<T> Handoff<T> {
    pub const fn new() -> Self {
        Self(Mutex::new(RefCell::new(None)))
    }

    pub fn put(&self, value: T) {
        critical_section::with(|cs| {
            *self.0.borrow_ref_mut(cs) = Some(value);
        });
    }

    /// Panics if [Self::put] was not called first.
    pub fn take(&self) -> T {
        critical_section::with(|cs| {
            self.0
                .borrow_ref_mut(cs)
                .take()
                .expect("Handoff value was not set before being taken")
        })
    }
}

impl<T> Default for Handoff<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[zynq7000_rt::irq]
pub fn irq_handler() {
    // Safety: Called here once.
    let result = unsafe { zynq7000_hal::generic_interrupt_handler() };
    if let Err(e) = result {
        panic!("Generic interrupt handler failed handling {:?}", e);
    }
}

#[zynq7000_rt::exception(DataAbort)]
fn data_abort_handler(_faulting_addr: usize) -> ! {
    loop {
        nop();
    }
}

#[zynq7000_rt::exception(Undefined)]
fn undefined_handler(_faulting_addr: usize) -> ! {
    loop {
        nop();
    }
}

#[zynq7000_rt::exception(PrefetchAbort)]
fn prefetch_handler(_faulting_addr: usize) -> ! {
    loop {
        nop();
    }
}

/// Panic handler. defmt takes the cross-core critical section for each message, so panics on
/// both cores do not interleave.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    defmt::error!("Panic: {}", defmt::Display2Format(info));
    loop {}
}
