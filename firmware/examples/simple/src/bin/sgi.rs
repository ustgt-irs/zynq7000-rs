//! Example which triggers a software generated interrupt (SGI) periodically.
#![no_std]
#![no_main]

use aarch32_cpu::asm::nop;
use core::{panic::PanicInfo, sync::atomic::AtomicU64};
use z7_defmt_rtt as _;
use zynq7000_hal::{
    BootMode,
    gic::{self, Configurator, Interrupt, SgiInterrupt},
    gpio,
};

static SGI_COUNTER: AtomicU64 = AtomicU64::new(0);

#[zynq7000_rt::entry]
fn main() -> ! {
    // The GIC is configured below, because the SGIs need to be routed to CPU0 as well.
    let mut config = zynq7000_hal::Config::default();
    config.interrupt_config = None;
    let dp = zynq7000_hal::init(config).unwrap();

    // Set up the global interrupt controller.
    let mut gic = Configurator::new_with_init(dp.gicc, dp.gicd);
    gic.enable_all_interrupts();
    gic.set_all_spi_interrupt_targets_cpu0();
    gic.set_all_sgi_interrupt_targets_cpu0();
    gic.enable();
    // Enable interrupt exception.
    unsafe { gic.enable_interrupts() };

    defmt::println!("-- Zynq 7000 Software Interrupt example --");
    let mio_pins = gpio::mio::Pins::new(dp.gpio);

    let boot_mode = BootMode::new_from_regs();
    // `BootMode` does not implement `defmt::Format`.
    defmt::info!("Boot mode: {:?}", defmt::Debug2Format(&boot_mode));

    let mut led = gpio::Output::new_for_mio(mio_pins.mio7, gpio::PinState::Low);
    loop {
        let gtc = SGI_COUNTER.load(core::sync::atomic::Ordering::Relaxed);
        defmt::info!("Hello, world!");
        defmt::info!("SGI counter: {}", gtc);
        gic.trigger_software_interrupt(SgiInterrupt::new(0).unwrap());
        led.toggle();
        for _ in 0..5_000_000 {
            nop();
        }
    }
}

#[zynq7000_rt::irq]
fn irq_handler() {
    let mut gic_helper = gic::InterruptGuard::new();
    let irq_info = gic_helper.interrupt_info();
    match irq_info.interrupt() {
        Interrupt::Sgi(_sgi) => {
            // TODO: Send ID to main thread.
            SGI_COUNTER.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        }
        Interrupt::Ppi(_ppi_interrupt) => (),
        Interrupt::Spi(_spi_interrupt) => (),
        Interrupt::Invalid(_) => (),
        Interrupt::Spurious => (),
    }
    gic_helper.end_of_interrupt(irq_info);
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

/// Panic handler
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    defmt::error!("Panic: {}", defmt::Display2Format(info));
    loop {}
}
