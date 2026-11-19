package io.github.poldion.tephra

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder

/// Hält die App im Vordergrund, solange Sitzungen offen sind. Ohne das friert Android sie wenige
/// Sekunden nach dem Wechsel in eine andere App ein und schließt dabei ihre Verbindungen.
class SessionService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (intent == null || intent.action == STOP) {
            stop()
            return START_NOT_STICKY
        }
        val notification = build(intent)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.UPSIDE_DOWN_CAKE) {
            startForeground(ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_SPECIAL_USE)
        } else {
            startForeground(ID, notification)
        }
        return START_NOT_STICKY
    }

    /// Wird die App aus der Übersicht gewischt, enden ihre Sitzungen; dann bleibt auch der Dienst nicht.
    override fun onTaskRemoved(rootIntent: Intent?) {
        stop()
    }

    private fun stop() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
            stopForeground(STOP_FOREGROUND_REMOVE)
        } else {
            @Suppress("DEPRECATION")
            stopForeground(true)
        }
        stopSelf()
    }

    private fun build(intent: Intent): Notification {
        val open = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(CHANNEL, intent.getStringExtra(CHANNEL_NAME), NotificationManager.IMPORTANCE_LOW)
            channel.setShowBadge(false)
            getSystemService(NotificationManager::class.java).createNotificationChannel(channel)
            Notification.Builder(this, CHANNEL)
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(this)
        }
        return builder
            .setSmallIcon(R.drawable.ic_stat_sessions)
            .setContentTitle(intent.getStringExtra(TITLE))
            .setContentText(intent.getStringExtra(TEXT))
            .setContentIntent(open)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setShowWhen(false)
            .build()
    }

    companion object {
        private const val ID = 4301
        private const val CHANNEL = "sessions"
        private const val STOP = "stop"
        private const val TITLE = "title"
        private const val TEXT = "text"
        private const val CHANNEL_NAME = "channel"

        /// Auftrag, den Dienst mit dieser Benachrichtigung zu starten oder sie zu erneuern.
        fun show(context: Context, title: String, text: String, channel: String): Intent =
            Intent(context, SessionService::class.java)
                .putExtra(TITLE, title)
                .putExtra(TEXT, text)
                .putExtra(CHANNEL_NAME, channel)

        /// Auftrag, den Dienst zu beenden.
        fun stop(context: Context): Intent = Intent(context, SessionService::class.java).setAction(STOP)
    }
}
