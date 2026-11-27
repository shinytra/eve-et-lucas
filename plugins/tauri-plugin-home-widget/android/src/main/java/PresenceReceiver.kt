package fr.eveetlucas.homewidget

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/**
 * Présence de l'autre reçue par ntfy (plugin geofence), même appli fermée :
 * le widget l'affiche tout de suite. Non exporté : seule cette appli peut
 * l'appeler.
 */
class PresenceReceiver : BroadcastReceiver() {
  companion object {
    const val ACTION_PRESENCE = "fr.eveetlucas.homewidget.ACTION_PRESENCE"
  }

  override fun onReceive(context: Context, intent: Intent) {
    if (intent.action != ACTION_PRESENCE) return
    val who = intent.getStringExtra("who")?.trim().orEmpty()
    val at = intent.getLongExtra("at", -1L)
    if (who.isEmpty() || at < 0L) return
    WidgetStore.savePresence(
      context,
      Presence(
        who,
        intent.getBooleanExtra("inside", false),
        intent.getStringExtra("place").orEmpty(),
        intent.getStringExtra("placeKind").orEmpty(),
        at
      )
    )
    HomeWidgetUpdater.refreshAll(context)
  }
}
