package fr.eveetlucas.homewidget

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

/**
 * Reçoit les cases cochées sur le widget, même appli fermée (Android démarre
 * alors le processus rien que pour ce récepteur).
 */
class WidgetActionReceiver : BroadcastReceiver() {
  companion object {
    const val ACTION_DONE = "fr.eveetlucas.homewidget.DONE"
    const val EXTRA_TASK_ID = "fr.eveetlucas.homewidget.TASK_ID"
  }

  override fun onReceive(context: Context, intent: Intent) {
    if (intent.action != ACTION_DONE) return
    val taskId = intent.getStringExtra(EXTRA_TASK_ID)
    if (taskId.isNullOrEmpty()) return

    // D'abord enregistrer : c'est ce que l'appli relira avec `peek_actions`.
    val action = WidgetStore.addDone(context, taskId, System.currentTimeMillis())
    try {
      HomeWidgetUpdater.refreshAll(context)
    } catch (e: Exception) {
      Log.w(TAG, "Widget non redessiné après la case cochée", e)
    }
    if (action != null) HomeWidgetPlugin.notifyAction(action)
  }
}
