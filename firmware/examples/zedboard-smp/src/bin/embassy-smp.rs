//! Embassy hello-world for the Zedboard, running an independent executor on each core.
//!
//! CPU0 blinks a MIO LED and sends its state to CPU1 over a cross-core `embassy_sync::Signal`,
//! which mirrors it on an EMIO LED. CPU1 also runs its own heartbeat task using
//! `embassy_time::Timer`, entirely independently of CPU0, to exercise the GTC time driver's
//! per-core alarm state. Both cores log with defmt.
#![no_std]
#![no_main]

use embassy_executor::Executor;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, signal::Signal};
use embassy_time::{Duration, Ticker};
use static_cell::StaticCell;
use z7_defmt_rtt as _;
use zedboard_smp::{Handoff, PS_CLOCK_FREQUENCY};
use zynq7000_hal::{BootMode, clocks::Clocks, gpio, gtc};

use zynq7000_rt as _;

/// Carries CPU0's LED state to CPU1's LED task. Only the latest state matters, so a `Signal`
/// fits better than a `Channel`. Sharing this between two independently-running executors is
/// safe: `critical_section` (multi-core feature) is a real cross-core spinlock, not just
/// IRQ-disable, and the `cortex_ar` executor wakes via a global `SEV` that reaches every core
/// sitting in `WFE`, regardless of which core produced the wakeup.
static LED_SIGNAL: Signal<CriticalSectionRawMutex, bool> = Signal::new();

/// CPU1's EMIO LED, constructed on CPU0 and handed off through this static before CPU1 is
/// released via `start_core1`.
static EMIO_LED: Handoff<gpio::Output> = Handoff::new();

static EXECUTOR0: StaticCell<Executor> = StaticCell::new();
static EXECUTOR1: StaticCell<Executor> = StaticCell::new();

/// Entry point on CPU0, called by `zynq7000-rt`'s start-up code.
#[zynq7000_rt::entry]
fn main() -> ! {
    let periphs = zynq7000_hal::init(zynq7000_hal::Config::default()).unwrap();
    let clocks = Clocks::new_from_regs(PS_CLOCK_FREQUENCY).unwrap();

    // Set up the global timer counter and embassy time driver. Both cores share this one
    // driver instance, but its alarm/queue state is tracked per-core internally, and each core
    // arms its own banked GTC comparator and interrupt.
    let gtc = gtc::GlobalTimerCounter::new(periphs.gtc, clocks.arm_clocks());
    zynq7000_hal::time_driver_gtc::init(clocks.arm_clocks(), gtc);

    let mut gpio_pins = gpio::Pins::new(periphs.gpio, gpio::Config::default());
    let mio_led = gpio::Output::new_for_mio(gpio_pins.mio.mio7, gpio::PinState::Low);

    // Construct CPU1's LED here and hand it off through EMIO_LED. This has to happen before
    // start_core1 below.
    let emio_led = gpio::Output::new_for_emio(gpio_pins.emio.take(0).unwrap(), gpio::PinState::Low);
    EMIO_LED.put(emio_led);

    defmt::println!("-- Zynq 7000 Zedboard Embassy SMP example --");

    let boot_mode = BootMode::new_from_regs();
    defmt::info!("Boot mode: {:?}", boot_mode);

    // Everything above touches SLCR/GPIO config, and we want to keep that on core0.
    zynq7000_rt::smp::start_core1();

    let executor0 = EXECUTOR0.init(Executor::new());
    executor0.run(|spawner| {
        spawner.spawn(core0_task(mio_led).unwrap());
    });
}

#[embassy_executor::task]
async fn core0_task(mut led: gpio::Output) -> ! {
    let mut ticker = Ticker::every(Duration::from_millis(500));
    let mut count = 0;
    loop {
        led.toggle();
        defmt::info!("CPU0 heartbeat: {}", count);
        count += 1;
        LED_SIGNAL.signal(led.is_set_high());
        ticker.next().await;
    }
}

/// Entry point on CPU1, called by `zynq7000-rt`'s start-up code once CPU0 has released it via
/// [`zynq7000_rt::smp::start_core1`].
#[unsafe(no_mangle)]
pub extern "C" fn kmain_secondary() {
    zynq7000_hal::init_secondary_core(zynq7000_hal::SecondaryCoreConfig::default()).unwrap();

    let emio_led = EMIO_LED.take();

    let executor1 = EXECUTOR1.init(Executor::new());
    executor1.run(|spawner| {
        spawner.spawn(core1_led_task(emio_led).unwrap());
        spawner.spawn(core1_heartbeat_task().unwrap());
    });
}

/// Mirrors CPU0's MIO LED state on CPU1's EMIO LED, driven by the cross-core signal.
#[embassy_executor::task]
async fn core1_led_task(mut led: gpio::Output) -> ! {
    loop {
        match LED_SIGNAL.wait().await {
            true => led.set_high(),
            false => led.set_low(),
        }
    }
}

/// Runs entirely independently of CPU0, using CPU1's own banked GTC comparator and interrupt.
#[embassy_executor::task]
async fn core1_heartbeat_task() -> ! {
    let mut ticker = Ticker::every(Duration::from_secs(1));
    let mut count = 0u32;
    loop {
        defmt::info!("CPU1 heartbeat: {}", count);
        count += 1;
        ticker.next().await;
    }
}
