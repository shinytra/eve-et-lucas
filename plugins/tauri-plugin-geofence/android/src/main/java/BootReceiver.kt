package fr.eveetlucas.geofence

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.util.Log

/**
 * Android efface les géofences au redémarrage (et à la mise à jour de
 * l'appli) : on réenregistre les lieux sauvegardés.
 */
class BootReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent) {
    val action = intent.action
    if (action != Intent.ACTION_BOOT_COMPLETED && action != Intent.ACTION_MY_PACKAGE_REPLACED) return
    val places = GeofenceStore.places(context)
    if (places.isEmpty()) return
    if (!GeofenceRegistrar.canGeofence(context)) {
      Log.w(TAG, "Autorisation de position manquante : lieux non réenregistrés")
      return
    }
    // Attend la réponse des services Google Play avant de rendre la main.
    val pending = goAsync()
    GeofenceRegistrar.register(
      context.applicationContext,
      places,
      checkOutside = true,
      onSuccess = {
        Log.i(TAG, "Lieux réenregistrés après $action")
        pending.finish()
      },
      onError = { message ->
        Log.w(TAG, "Lieux non réenregistrés : $message")
        pending.finish()
      }
    )
  }
}
