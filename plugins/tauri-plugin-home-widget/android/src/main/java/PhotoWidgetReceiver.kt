package fr.eveetlucas.homewidget

import android.appwidget.AppWidgetManager
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/**
 * Toucher le widget photo passe à l'image suivante, même appli fermée.
 * Non exporté : seul le PendingIntent du widget peut l'appeler.
 */
class PhotoWidgetReceiver : BroadcastReceiver() {
  companion object {
    const val ACTION_NEXT = "fr.eveetlucas.homewidget.PHOTO_NEXT"
  }

  override fun onReceive(context: Context, intent: Intent) {
    if (intent.action != ACTION_NEXT) return
    val id = intent.getIntExtra(AppWidgetManager.EXTRA_APPWIDGET_ID, AppWidgetManager.INVALID_APPWIDGET_ID)
    if (id == AppWidgetManager.INVALID_APPWIDGET_ID) return
    PhotoWidgetStore.next(context, id)
    val pending = goAsync()
    PhotoWidgetUpdater.updateAsync(context, intArrayOf(id)) { pending.finish() }
  }
}
