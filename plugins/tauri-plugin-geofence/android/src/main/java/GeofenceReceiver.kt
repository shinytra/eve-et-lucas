package fr.eveetlucas.geofence

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import com.google.android.gms.location.Geofence
import com.google.android.gms.location.GeofencingEvent

/**
 * Reçoit les évènements de la géofence « maison », même appli fermée
 * (Android démarre alors le processus rien que pour ce récepteur).
 */
class GeofenceReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent) {
    if (intent.action != GeofenceRegistrar.ACTION_GEOFENCE) return
    val event = GeofencingEvent.fromIntent(intent) ?: return
    if (event.hasError()) {
      Log.w(TAG, "Erreur de géofence, code ${event.errorCode}")
      return
    }
    if (event.geofenceTransition != Geofence.GEOFENCE_TRANSITION_ENTER) return

    val at = System.currentTimeMillis()
    // D'abord enregistrer : c'est ce que l'appli relira avec `take_arrivals`.
    GeofenceStore.addArrival(context, at)
    ArrivalNotifier.show(context)
    GeofencePlugin.notifyArrival(at)

    if (GeofenceStore.ringOnArrival(context)) {
      // Garde le récepteur en vie le temps de la sonnerie (moins de 10 s).
      val pending = goAsync()
      ArrivalNotifier.ring(context.applicationContext) { pending.finish() }
    }
  }
}
