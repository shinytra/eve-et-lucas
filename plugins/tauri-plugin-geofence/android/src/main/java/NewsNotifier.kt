package fr.eveetlucas.geofence

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.os.Build
import android.util.Log
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat

/**
 * Notifications des nouveautés (note ajoutée, tâche faite…) reçues par ntfy,
 * même appli fermée. Canal à part de celui des arrivées : on peut lui donner
 * un son plus discret dans les réglages d'Android.
 */
object NewsNotifier {
  const val CHANNEL_ID = "nouveautes"
  /** Une notification par élément (étiquette) : l'épisode suivant remplace le précédent. */
  private const val NOTIFICATION_ID = 4244

  private fun createChannel(context: Context) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
    val channel = NotificationChannel(
      CHANNEL_ID,
      "Nouveautés",
      NotificationManager.IMPORTANCE_DEFAULT
    )
    channel.description = "Notes, tâches, dates, animés et fichiers ajoutés par l'autre."
    val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
    manager.createNotificationChannel(channel)
  }

  fun show(context: Context, event: EventMessage) {
    createChannel(context)
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
      !GeofenceRegistrar.hasPermission(context, Manifest.permission.POST_NOTIFICATIONS)
    ) {
      Log.i(TAG, "Notifications non autorisées : nouveauté non affichée")
      return
    }
    val builder = NotificationCompat.Builder(context, CHANNEL_ID)
      .setSmallIcon(android.R.drawable.ic_popup_reminder)
      .setContentTitle(event.title)
      .setContentText(event.body)
      .setPriority(NotificationCompat.PRIORITY_DEFAULT)
      .setCategory(NotificationCompat.CATEGORY_SOCIAL)
      .setWhen(event.at)
      .setShowWhen(true)
      .setAutoCancel(true)
    // Toucher la notification ouvre l'appli (qui se synchronise).
    context.packageManager.getLaunchIntentForPackage(context.packageName)?.let { launch ->
      val flags = PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
      builder.setContentIntent(PendingIntent.getActivity(context, 0, launch, flags))
    }
    val tag = "${event.kind}:${event.item.ifEmpty { event.id }}"
    try {
      NotificationManagerCompat.from(context).notify(tag, NOTIFICATION_ID, builder.build())
    } catch (e: SecurityException) {
      Log.w(TAG, "Notification de nouveauté refusée par Android", e)
    }
  }
}
