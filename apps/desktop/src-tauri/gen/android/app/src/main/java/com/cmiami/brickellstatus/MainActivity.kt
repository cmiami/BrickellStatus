package com.cmiami.brickellstatus

import android.Manifest
import android.content.pm.PackageManager
import android.graphics.Color
import android.os.Build
import android.os.Bundle
import androidx.activity.SystemBarStyle
import androidx.activity.enableEdgeToEdge
import androidx.activity.result.contract.ActivityResultContracts
import androidx.core.content.ContextCompat

class MainActivity : TauriActivity() {
  // Registered as a field so it is in place before the activity is STARTED,
  // which is the only window in which a result launcher may be created.
  private val permissionLauncher =
    registerForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
      // The watch starts before this resolves, so on a first launch it can only
      // claim the dataSync type. Restarting once the answer is in lets it add
      // connectedDevice, which is what keeps the panel link alive in the
      // background. Idempotent, so a refusal simply leaves it as it was.
      WatchService.start(this)
    }

  override fun onCreate(savedInstanceState: Bundle?) {
    // The console is a light interface with no dark variant, so both system
    // bars are forced to light appearance — meaning dark icons. The default
    // enableEdgeToEdge() follows the system theme, which on a phone in dark
    // mode paints white icons onto the console's near-white header and hides
    // the clock, signal and battery entirely. Transparent scrims because the
    // header reserves the inset itself and draws its own background there.
    enableEdgeToEdge(
      statusBarStyle = SystemBarStyle.light(Color.TRANSPARENT, Color.TRANSPARENT),
      navigationBarStyle = SystemBarStyle.light(Color.TRANSPARENT, Color.TRANSPARENT)
    )
    // Before super.onCreate, which is where Tauri starts the Rust main loop on
    // a thread of its own. See NativeBridge for why the class loader makes the
    // ordering matter.
    NativeBridge.initBluetooth()
    NativeBridge.initStatusBridge()
    super.onCreate(savedInstanceState)
    requestRuntimePermissions()
    // Started from the activity because Android forbids launching a foreground
    // service from the background. From here on the process outlives this
    // activity, which is what keeps the collectors running once the app is
    // swiped away.
    WatchService.start(this)
  }

  /**
   * Asks for the permissions that are not granted at install time.
   *
   * A refusal is not handled here: a denied Bluetooth permission surfaces
   * through btleplug as an ordinary transport error, and a denied notification
   * permission makes the notification silently no-op, both of which the app
   * already reports in its own terms.
   */
  private fun requestRuntimePermissions() {
    val wanted = buildList {
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        add(Manifest.permission.BLUETOOTH_SCAN)
        add(Manifest.permission.BLUETOOTH_CONNECT)
      } else {
        // Below API 31 a BLE scan is treated as a location capability.
        add(Manifest.permission.ACCESS_FINE_LOCATION)
      }
      if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        add(Manifest.permission.POST_NOTIFICATIONS)
      }
    }.filter {
      ContextCompat.checkSelfPermission(this, it) != PackageManager.PERMISSION_GRANTED
    }
    if (wanted.isNotEmpty()) {
      permissionLauncher.launch(wanted.toTypedArray())
    }
  }
}
