package fr.eveetlucas.geofence

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import java.util.concurrent.atomic.AtomicInteger
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
    // D'abord enregistrer : c'est ce que l'appli relira avec `peek_arrivals`.
    val arrival = GeofenceStore.recordArrival(context, at)
    if (arrival == null) {
      Log.i(TAG, "Entrée dans la zone moins de 5 min après la précédente : ignorée")
      return
    }
    // Ce téléphone n'a pas à sonner une deuxième fois pour sa propre arrivée.
    GeofenceStore.markRung(context, listOf(arrival.id))
    ArrivalNotifier.show(context)
    GeofencePlugin.notifyArrival(arrival)

    val ring = GeofenceStore.ringOnArrival(context)
    val config = GeofenceStore.pushConfig(context)
    // Sans prénom, les autres refuseraient le message : inutile de l'envoyer.
    val push = PushCrypto.decodeKey(config.key) != null &&
      config.who.isNotBlank() &&
      PushSender.validTargets(config.targets).isNotEmpty()
    if (!ring && !push) return

    // Garde le récepteur en vie le temps de la sonnerie et de l'envoi (~10 s
    // au plus, les deux en parallèle) ; `finish()` est appelé une seule fois,
    // quand tout est terminé, quoi qu'il arrive.
    val pending = goAsync()
    val remaining = AtomicInteger(1 + (if (ring) 1 else 0) + (if (push) 1 else 0))
    val done = {
      if (remaining.decrementAndGet() == 0) pending.finish()
    }
    try {
      if (ring) {
        try {
          ArrivalNotifier.ring(context.applicationContext, done)
        } catch (e: Exception) {
          Log.w(TAG, "Sonnerie impossible", e)
          done()
        }
      }
      if (push) {
        val message = ArrivalMessage(arrival.id, config.who, config.device, arrival.at)
        try {
          PushSender.send(config, message, done)
        } catch (e: Exception) {
          Log.w(TAG, "Envoi ntfy impossible", e)
          done()
        }
      }
    } finally {
      done()
    }
  }
}
