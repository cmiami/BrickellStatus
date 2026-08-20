package com.cmiami.brickellstatus

/**
 * Rust entry points that Java has to call directly, rather than letting Rust
 * reach for them on its own.
 *
 * btleplug's Android backend finds its Java classes with the JNI `FindClass`
 * call, which searches the class loader belonging to the nearest Java frame on
 * the calling thread's stack. Only a thread that Java called into carries the
 * app's own class loader; a thread Rust spawned carries the system loader,
 * which cannot see anything packaged in this APK. Tauri runs the Rust main loop
 * on a spawned thread, so the Bluetooth handshake has to happen here first.
 */
object NativeBridge {
    init {
        // Matches `[lib] name` in apps/desktop/src-tauri/Cargo.toml. Loading is
        // idempotent, so Tauri's own load of the same library later is a no-op.
        System.loadLibrary("brickellstatus_desktop_lib")
    }

    /**
     * Hands btleplug the JVM handle it needs before any BLE call.
     *
     * Never throws: the Rust side records the outcome and reports a missing
     * Bluetooth adapter afterwards, so a device without Bluetooth loses the
     * e-paper output and nothing else.
     */
    @JvmStatic
    external fun initBluetooth()

    /**
     * Caches what Rust needs to update the watch notification later.
     *
     * Same class-loader constraint as [initBluetooth]: the lookup only resolves
     * from a thread the JVM called into, so it happens here rather than lazily
     * from whichever worker first has something to say.
     */
    @JvmStatic
    external fun initStatusBridge()
}
