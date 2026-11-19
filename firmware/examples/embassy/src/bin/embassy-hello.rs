#![no_std]
#![no_main]

use aarch32_cpu::asm::nop;
use core::panic::PanicInfo;
use embassy_executor::Spawner;
use embassy_time::{Duration, Ticker};
use z7_defmt_rtt as _;
use zynq7000_hal::{BootMode, clocks, generic_interrupt_handler, gpio, gtc, time::Hertz};

use zynq7000_rt as _;

// Define the clock frequency as a constant
const PS_CLOCK_FREQUENCY: Hertz = Hertz::from_raw(33_333_300);

/// Entry point which calls the embassy main method.
#[zynq7000_rt::entry]
fn entry_point() -> ! {
    main();
}

#[embassy_executor::main]
async fn main(_spawner: Spawner) -> ! {
    let periphs = zynq7000_hal::init(zynq7000_hal::Config::default()).unwrap();
    let clocks = clocks::Clocks::new_from_regs(PS_CLOCK_FREQUENCY).unwrap();

    // Set up global timer counter and embassy time driver.
    let gtc = gtc::GlobalTimerCounter::new(periphs.gtc, clocks.arm_clocks());
    zynq7000_hal::time_driver_gtc::init(clocks.arm_clocks(), gtc);

    let mio_pins = gpio::mio::Pins::new(periphs.gpio);

    let boot_mode = BootMode::new_from_regs();
    defmt::info!("Boot mode: {:?}", boot_mode);

    let mut ticker = Ticker::every(Duration::from_millis(1000));
    let mut led = gpio::Output::new_for_mio(mio_pins.mio7, gpio::PinState::Low);
    loop {
        defmt::info!("Hello, world!");
        led.toggle();
        ticker.next().await;
    }
}

#[zynq7000_rt::irq]
pub fn irq_handler() {
    // Safety: Called here once.
    let result = unsafe { generic_interrupt_handler() };
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

/// Panic handler
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    defmt::error!("Panic: {}", defmt::Display2Format(info));
    loop {}
}
