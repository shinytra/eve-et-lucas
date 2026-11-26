package fr.eveetlucas.geofence

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.media.AudioAttributes
import android.media.RingtoneManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat

/** Notification et sonnerie de l'arrivée à la maison. */
object ArrivalNotifier {
  const val CHANNEL_ID = "arrivee"
  private const val NOTIFICATION_ID = 4242

  /** Durée de la sonnerie quand « sonner à l'arrivée » est activé. */
  private const val RING_DURATION_MS = 5_000L

  fun createChannel(context: Context) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
    val channel = NotificationChannel(
      CHANNEL_ID,
      "Arrivée à la maison",
      NotificationManager.IMPORTANCE_HIGH
    )
    channel.description = "Prévient quand ce téléphone arrive à la maison."
    val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
    manager.createNotificationChannel(channel)
  }

  fun show(context: Context) {
    createChannel(context)
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
      !GeofenceRegistrar.hasPermission(context, Manifest.permission.POST_NOTIFICATIONS)
    ) {
      Log.i(TAG, "Notifications non autorisées : arrivée enregistrée sans notification")
      return
    }
    val builder = NotificationCompat.Builder(context, CHANNEL_ID)
      .setSmallIcon(android.R.drawable.ic_menu_mylocation)
      .setContentTitle("Arrivée à la maison")
      .setContentText("Bienvenue à la maison !")
      .setPriority(NotificationCompat.PRIORITY_HIGH)
      .setCategory(NotificationCompat.CATEGORY_STATUS)
      .setAutoCancel(true)
    // Toucher la notification ouvre l'appli.
    context.packageManager.getLaunchIntentForPackage(context.packageName)?.let { launch ->
      val flags = PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
      builder.setContentIntent(PendingIntent.getActivity(context, 0, launch, flags))
    }
    try {
      NotificationManagerCompat.from(context).notify(NOTIFICATION_ID, builder.build())
    } catch (e: SecurityException) {
      Log.w(TAG, "Notification d'arrivée refusée par Android", e)
    }
  }

  /** Fait sonner le téléphone quelques secondes, puis appelle [done]. */
  fun ring(context: Context, done: () -> Unit) {
    val uri = RingtoneManager.getDefaultUri(RingtoneManager.TYPE_ALARM)
      ?: RingtoneManager.getDefaultUri(RingtoneManager.TYPE_NOTIFICATION)
    val ringtone = uri?.let { RingtoneManager.getRingtone(context, it) }
    if (ringtone == null) {
      Log.w(TAG, "Aucune sonnerie disponible")
      done()
      return
    }
    try {
      ringtone.setAudioAttributes(
        AudioAttributes.Builder()
          .setUsage(AudioAttributes.USAGE_ALARM)
          .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
          .build()
      )
      ringtone.play()
    } catch (e: Exception) {
      Log.w(TAG, "Impossible de faire sonner le téléphone", e)
      done()
      return
    }
    Handler(Looper.getMainLooper()).postDelayed({
      try {
        ringtone.stop()
      } catch (e: Exception) {
        Log.w(TAG, "Arrêt de la sonnerie impossible", e)
      }
      done()
    }, RING_DURATION_MS)
  }
}
