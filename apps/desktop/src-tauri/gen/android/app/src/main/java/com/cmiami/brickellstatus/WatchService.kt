package com.cmiami.brickellstatus

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder
import android.os.PowerManager
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat

/**
 * Keeps the collection runtime alive while BrickellStatus is not on screen.
 *
 * The Rust workers are plain tokio tasks owned by the process, not by the
 * activity, so they keep ticking for exactly as long as the process is allowed
 * to run. Backgrounded without this service the process is cached and then
 * frozen, which stops the collectors, the notification dispatch and the frames
 * going out to the e-paper panel — leaving an advance-warning app that only
 * warns while you are already looking at it.
 *
 * A foreground service is what buys the process out of that: it is not frozen,
 * it keeps network access under Doze, and `connectedDevice` is the type that
 * permits an open BLE connection while backgrounded.
 */
class WatchService : Service() {
    private var wakeLock: PowerManager.WakeLock? = null

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        createChannel()
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent?.action == ACTION_STOP) {
            // The user asked for it from the notification. Stop watching rather
            // than letting the service restart itself.
            stopSelf()
            return START_NOT_STICKY
        }

        try {
            ServiceCompat.startForeground(
                this,
                NOTIFICATION_ID,
                buildFrom(this, status, detail),
                allowedType()
            )
        } catch (error: Exception) {
            // Two ways this legitimately fails, and neither is worth a crash:
            // the connectedDevice type is refused when the Bluetooth grant is
            // missing, and any foreground start is refused outright when the
            // app is already in the background. Give up on watching rather than
            // taking down a process the user may be looking at. START_NOT_STICKY
            // so Android does not retry into the same wall.
            Log.w(TAG, "Could not start the river watch in the background", error)
            stopSelf()
            return START_NOT_STICKY
        }

        if (wakeLock == null) {
            // Doze parks timers between maintenance windows, which for a
            // five-second poll means the warning arrives after the bridge has
            // already opened. The cost is real battery; the notification
            // carries a Stop action so it is the user's call to keep paying it.
            val power = getSystemService(Context.POWER_SERVICE) as PowerManager
            wakeLock = power.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, WAKE_LOCK_TAG).apply {
                setReferenceCounted(false)
                acquire()
            }
        }
        // START_STICKY so Android brings the watch back after killing it for
        // memory, which is the case this whole service exists to survive.
        return START_STICKY
    }

    override fun onDestroy() {
        wakeLock?.let { if (it.isHeld) it.release() }
        wakeLock = null
        super.onDestroy()
    }

    /**
     * The service types this process is currently *allowed* to claim.
     *
     * `connectedDevice` is refused unless a Bluetooth runtime permission is
     * already granted — declaring it in the manifest is not enough, and asking
     * for it anyway throws SecurityException. On a first launch the permission
     * dialog is still on screen, so the watch starts as `dataSync` alone and
     * MainActivity restarts it once the user has answered.
     */
    private fun allowedType(): Int {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.UPSIDE_DOWN_CAKE) return 0
        var type = ServiceInfo.FOREGROUND_SERVICE_TYPE_DATA_SYNC
        if (hasBluetoothGrant(this)) {
            type = type or ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE
        }
        return type
    }

    private fun createChannel() {
        val channel = NotificationChannel(
            CHANNEL_ID,
            getString(R.string.watch_channel_name),
            // LOW: ongoing and glanceable, never a sound or a heads-up. The
            // alerts that matter come through the app's own notifications.
            NotificationManager.IMPORTANCE_LOW
        ).apply {
            description = getString(R.string.watch_channel_description)
            setShowBadge(false)
        }
        (getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager)
            .createNotificationChannel(channel)
    }

    companion object {
        private const val CHANNEL_ID = "brickellstatus.watch"
        private const val NOTIFICATION_ID = 1
        private const val ACTION_STOP = "com.cmiami.brickellstatus.STOP_WATCH"
        private const val WAKE_LOCK_TAG = "brickellstatus:watch"
        private const val TAG = "BrickellStatusWatch"

        /**
         * Whether a Bluetooth runtime permission is held. Below API 31 the old
         * install-time BLUETOOTH permission applies and there is nothing to ask
         * for at runtime.
         */
        @JvmStatic
        fun hasBluetoothGrant(context: Context): Boolean {
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return true
            return ContextCompat.checkSelfPermission(context, Manifest.permission.BLUETOOTH_CONNECT) ==
                PackageManager.PERMISSION_GRANTED
        }

        // Last text published by the Rust runtime, so a service restart redraws
        // the status it was showing rather than the generic starting line.
        @Volatile private var status: String = "BrickellStatus"
        @Volatile private var detail: String = "Watching the river."
        @Volatile private var appContext: Context? = null

        /** Starts watching. Safe to call repeatedly. */
        @JvmStatic
        fun start(context: Context) {
            appContext = context.applicationContext
            val intent = Intent(context, WatchService::class.java)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                context.startForegroundService(intent)
            } else {
                context.startService(intent)
            }
        }

        /**
         * Replaces the ongoing notification's text. Called from Rust whenever
         * the decision changes, which turns the cost of a permanent
         * notification into the cheapest way to read the river.
         *
         * A no-op before the service has started, and deliberately quiet on
         * failure: a status line is never worth taking the process down for.
         */
        @JvmStatic
        fun publishStatus(title: String, body: String) {
            status = title
            detail = body
            val context = appContext ?: return
            try {
                val manager =
                    context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
                // Nothing to update before the service has built its channel.
                if (manager.getNotificationChannel(CHANNEL_ID) == null) return
                manager.notify(NOTIFICATION_ID, buildFrom(context, title, body))
            } catch (_: Throwable) {
            }
        }

        private fun buildFrom(context: Context, title: String, body: String): Notification {
            val open = PendingIntent.getActivity(
                context,
                0,
                Intent(context, MainActivity::class.java)
                    .setFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
            )
            val stop = PendingIntent.getService(
                context,
                1,
                Intent(context, WatchService::class.java).setAction(ACTION_STOP),
                PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT
            )
            return NotificationCompat.Builder(context, CHANNEL_ID)
                .setSmallIcon(R.drawable.ic_stat_watch)
                .setContentTitle(title)
                .setContentText(body)
                .setStyle(NotificationCompat.BigTextStyle().bigText(body))
                .setContentIntent(open)
                .addAction(0, context.getString(R.string.watch_stop), stop)
                .setOngoing(true)
                .setSilent(true)
                .setShowWhen(false)
                .setCategory(NotificationCompat.CATEGORY_SERVICE)
                .setPriority(NotificationCompat.PRIORITY_LOW)
                .build()
        }
    }
}
