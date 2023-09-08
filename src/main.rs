use std::error::Error;
use std::ffi::{c_char, CStr};
use std::mem;
use esp_idf_hal::gpio::{InterruptType, PinDriver, Pull};
use esp_idf_hal::prelude::Peripherals;
use esp_idf_svc::eventloop::{
    EspEventFetchData, EspEventPostData, EspSystemEventLoop, EspTypedEventDeserializer,
    EspTypedEventSerializer, EspTypedEventSource,
};

fn main() -> Result<(), Box<dyn Error>> {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_sys::link_patches();
    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    // 1. Take hold of the system event loop.
    let system_loop = EspSystemEventLoop::take()?;

    // 2. Subscribe to our custom message type and continue to run it beyond main().
    let subscription = system_loop.subscribe(|message: &CustomEvent| {
        log::info!("message coming in: {:?}", message);
    })?;
    mem::forget(subscription);

    // 3. Setup ISR for boot button. Same trick as above: Forgetting about it keeps it alive :^)
    // NOTE: The entire `PinDriver` needs to be kept alive; If it gets dropped, the subscription
    //       goes with it!
    let peripherals = Peripherals::take().expect("Peripherals inaccessible!");
    let mut boot_button = PinDriver::input(peripherals.pins.gpio0)?;
    boot_button.set_pull(Pull::Up)?;
    boot_button.set_interrupt_type(InterruptType::PosEdge)?;
    unsafe {
        // SAFETY: The callback (the ISR) lives in IRAM, not in DRAM. We're using the event loop
        //         as an isolation layer, retrieving the message safely from DRAM in the
        //         event loop's subscription above. stdlib, log, etc. are a big no-no here.
        boot_button.subscribe(move || {
            // We can only post to an event loop if it's allowed using
            // `CONFIG_ESP_EVENT_POST_FROM_ISR=y`.
            system_loop.post(&CustomEvent::Boop, None)
                .expect("ISR could not post message!");
        })?;
    }
    mem::forget(boot_button);

    log::info!("System ready to roll! Have fun, press some button!");
    Ok(())
}

/// Custom events
///
/// This seemed to be the most straight forward way to do 'em. If there's a better way,
/// I'm open to suggestions.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum CustomEvent {
    /// >///<!
    Boop,
}

impl EspTypedEventSource for CustomEvent {
    fn source() -> *const c_char {
        CStr::from_bytes_with_nul(b"CUSTOM\0").unwrap().as_ptr()
    }
}

impl EspTypedEventDeserializer<CustomEvent> for CustomEvent {
    fn deserialize<R>(data: &EspEventFetchData, f: &mut impl for<'a> FnMut(&'a CustomEvent) -> R) -> R {
        unsafe { f(data.as_payload()) }
    }
}

impl EspTypedEventSerializer<CustomEvent> for CustomEvent {
    fn serialize<R>(payload: &CustomEvent, f: impl for<'a> FnOnce(&'a EspEventPostData) -> R) -> R {
        let data;
        unsafe {
            data = EspEventPostData::new(
                CustomEvent::source(),
                CustomEvent::event_id(),
                payload,
            );
        }
        f(&data)
    }
}
