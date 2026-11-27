package fr.eveetlucas.geofence

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log
import java.util.concurrent.atomic.AtomicInteger
import com.google.android.gms.location.Geofence
import com.google.android.gms.location.GeofencingEvent

/**
 * Reçoit les entrées et sorties des lieux, même appli fermée (Android
 * démarre alors le processus rien que pour ce récepteur).
 */
class GeofenceReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent) {
    if (intent.action != GeofenceRegistrar.ACTION_GEOFENCE) return
    val event = GeofencingEvent.fromIntent(intent) ?: return
    if (event.hasError()) {
      Log.w(TAG, "Erreur de géofence, code ${event.errorCode}")
      return
    }
    val ids = event.triggeringGeofences.orEmpty().map { it.requestId }
    val none = emptyList<String>()
    val entered: List<String>
    val exited: List<String>
    when (event.geofenceTransition) {
      Geofence.GEOFENCE_TRANSITION_ENTER -> {
        entered = ids
        exited = none
      }
      Geofence.GEOFENCE_TRANSITION_EXIT -> {
        entered = none
        exited = ids
      }
      else -> return
    }

    // D'abord enregistrer : c'est ce que l'appli relira avec `peek_arrivals`.
    val change = GeofenceStore.recordTransition(
      context,
      System.currentTimeMillis(),
      entered,
      exited,
      allowArrival = true
    )
    if (change == null) {
      Log.i(TAG, "Entrée ou sortie sans changement de lieu : rien à annoncer")
      return
    }
    val arrival = change.kind == KIND_ARRIVAL
    if (arrival) {
      // Ce téléphone n'a pas à sonner une deuxième fois pour sa propre arrivée.
      GeofenceStore.markRung(context, listOf(change.id))
      ArrivalNotifier.show(context, change)
    }
    GeofencePlugin.notifyArrival(change)

    val ring = arrival && GeofenceStore.ringOnArrival(context)
    val push = PresenceAnnouncer.canPush(GeofenceStore.pushConfig(context))
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
      if (push) PresenceAnnouncer.push(context, change, done)
    } finally {
      done()
    }
  }
}
