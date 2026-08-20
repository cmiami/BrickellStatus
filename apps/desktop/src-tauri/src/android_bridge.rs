//! The Java-to-Rust entry points the Android build needs, and the one call that
//! goes back the other way.
//!
//! btleplug's Android backend looks its Java classes up with the JNI
//! `FindClass` call, which resolves against the class loader of the nearest
//! Java frame on the calling thread's stack. A thread Rust spawned carries only
//! the system loader, which cannot see classes packaged in the APK. Tauri runs
//! the Rust main loop on a spawned thread, so anything needing app classes has
//! to be reached from a thread Java called into — which is why
//! `MainActivity.onCreate` calls into here rather than the other way round.
//!
//! The same constraint shapes [`publish_status`]: the class reference it needs
//! is resolved once, during the Java-called init, and kept as a global ref so
//! the collection workers can use it later from their own threads.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use brickellstatus_eink::jni::objects::{GlobalRef, JClass, JValue};
use brickellstatus_eink::jni::{JNIEnv, JavaVM};
use tracing::{info, warn};

/// Whether the Bluetooth handshake ran and succeeded.
///
/// `btleplug::platform::global_adapter()` panics when it did not, so every path
/// that reaches the platform adapter consults this first and reports a missing
/// adapter instead of taking the process down.
static BLUETOOTH_READY: AtomicBool = AtomicBool::new(false);

/// Everything needed to call back into Kotlin from a worker thread.
struct StatusBridge {
    vm: JavaVM,
    watch_service: GlobalRef,
}

static STATUS_BRIDGE: OnceLock<StatusBridge> = OnceLock::new();

/// Last text handed to Android. The dispatch worker ticks every five seconds
/// and the decision usually has not moved, so this keeps the common case to a
/// string comparison instead of a JNI round trip and a notification redraw.
static LAST_PUBLISHED: Mutex<Option<(String, String)>> = Mutex::new(None);

/// Whether BLE calls are safe to make on this device.
pub fn bluetooth_ready() -> bool {
    BLUETOOTH_READY.load(Ordering::Acquire)
}

/// Initialises btleplug against the running JVM.
///
/// Declared on `com.cmiami.brickellstatus.NativeBridge` and called once from
/// `MainActivity.onCreate`. Failure is recorded rather than propagated: a phone
/// with Bluetooth switched off, or a build whose Java companion did not make it
/// into the APK, should lose the e-paper output and nothing else.
///
/// # Safety
///
/// Called by the JVM with a valid environment pointer for the current thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cmiami_brickellstatus_NativeBridge_initBluetooth(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
) {
    match brickellstatus_eink::transport::init_android_bluetooth(&env) {
        Ok(()) => {
            BLUETOOTH_READY.store(true, Ordering::Release);
            info!("Android Bluetooth bridge ready");
        }
        Err(error) => {
            warn!(%error, "Android Bluetooth bridge unavailable; the e-paper panel cannot be reached");
        }
    }
}

/// Caches what [`publish_status`] needs to reach `WatchService` later.
///
/// Must run on a thread the JVM called into: the class lookup here is the whole
/// point of the exercise, and it only resolves against the app's class loader
/// from such a thread.
///
/// # Safety
///
/// Called by the JVM with a valid environment pointer for the current thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_com_cmiami_brickellstatus_NativeBridge_initStatusBridge(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
) {
    let prepared = (|| -> Result<StatusBridge, String> {
        let class = env
            .find_class("com/cmiami/brickellstatus/WatchService")
            .map_err(|error| error.to_string())?;
        Ok(StatusBridge {
            vm: env.get_java_vm().map_err(|error| error.to_string())?,
            watch_service: env
                .new_global_ref(class)
                .map_err(|error| error.to_string())?,
        })
    })();
    match prepared {
        Ok(bridge) => {
            let _ = STATUS_BRIDGE.set(bridge);
            info!("Android status bridge ready");
        }
        Err(error) => {
            warn!(%error, "Android status bridge unavailable; the ongoing notification will not track the river");
        }
    }
}

/// Rewrites the watch notification so the running status is readable without
/// opening the app.
///
/// A no-op off Android and before the bridge is initialised. Failures are
/// logged and swallowed: a status line is never worth failing a dispatch tick
/// over, let alone taking the process down.
pub fn publish_status(title: &str, body: &str) {
    let Some(bridge) = STATUS_BRIDGE.get() else {
        return;
    };
    {
        let mut last = LAST_PUBLISHED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if last
            .as_ref()
            .is_some_and(|(seen_title, seen_body)| seen_title == title && seen_body == body)
        {
            return;
        }
        *last = Some((title.to_owned(), body.to_owned()));
    }
    let published = (|| -> Result<(), String> {
        // Attaching per call rather than holding an attachment: the guard drops
        // the thread's local references on the way out, and a worker that runs
        // for the life of the process would otherwise accumulate one JNI local
        // ref per tick forever.
        let env = bridge
            .vm
            .attach_current_thread()
            .map_err(|error| error.to_string())?;
        let title = env.new_string(title).map_err(|error| error.to_string())?;
        let body = env.new_string(body).map_err(|error| error.to_string())?;
        env.call_static_method(
            JClass::from(bridge.watch_service.as_obj()),
            "publishStatus",
            "(Ljava/lang/String;Ljava/lang/String;)V",
            &[JValue::Object(title.into()), JValue::Object(body.into())],
        )
        .map_err(|error| error.to_string())?;
        Ok(())
    })();
    if let Err(error) = published {
        warn!(%error, "watch notification status update failed");
    }
}
