package fr.eveetlucas.geofence

import android.Manifest
import android.annotation.SuppressLint
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.content.ContextCompat
import com.google.android.gms.common.api.ApiException
import com.google.android.gms.location.Geofence
import com.google.android.gms.location.GeofenceStatusCodes
import com.google.android.gms.location.GeofencingRequest
import com.google.android.gms.location.LocationServices

/**
 * Enregistrement des zones des lieux (une géofence par lieu, identifiée par
 * l'identifiant du lieu) auprès des services Google Play. Pas de GPS en
 * continu : Android surveille les zones lui-même, au moindre coût.
 */
object GeofenceRegistrar {
  /** Identifiant de l'ancienne géofence « maison » unique, gardé pour ce lieu. */
  const val LEGACY_ID = "maison"
  const val ACTION_GEOFENCE = "fr.eveetlucas.geofence.ACTION_GEOFENCE"

  fun hasPermission(context: Context, permission: String): Boolean =
    ContextCompat.checkSelfPermission(context, permission) == PackageManager.PERMISSION_GRANTED

  /** Position précise et, sur Android 10+, en arrière-plan. */
  fun canGeofence(context: Context): Boolean =
    hasPermission(context, Manifest.permission.ACCESS_FINE_LOCATION) &&
      (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q ||
        hasPermission(context, Manifest.permission.ACCESS_BACKGROUND_LOCATION))

  /** Message expliquant ce qui manque, ou `null` si tout est accordé. */
  fun missingPermissionMessage(context: Context): String? {
    if (!hasPermission(context, Manifest.permission.ACCESS_FINE_LOCATION)) {
      return "Autorisez d'abord la position précise : elle sert à savoir dans quel lieu vous êtes."
    }
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q &&
      !hasPermission(context, Manifest.permission.ACCESS_BACKGROUND_LOCATION)
    ) {
      return "Choisissez « Toujours autoriser » pour la position : sans cela, les entrées et sorties ne sont pas détectées quand l'appli est fermée."
    }
    return null
  }

  private fun pendingIntent(context: Context): PendingIntent {
    val intent = Intent(context, GeofenceReceiver::class.java).setAction(ACTION_GEOFENCE)
    // Android 12+ : le PendingIntent d'une géofence doit être modifiable
    // (les services Google Play y ajoutent l'évènement).
    var flags = PendingIntent.FLAG_UPDATE_CURRENT
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
      flags = flags or PendingIntent.FLAG_MUTABLE
    }
    return PendingIntent.getBroadcast(context.applicationContext, 0, intent, flags)
  }

  /**
   * Enregistre (ou remplace) les zones de [places], entrées et sorties. Les
   * rappels arrivent sur le fil principal.
   *
   * [checkOutside] : à la réinscription (lancement, redémarrage), Android
   * signale aussitôt une sortie pour chaque zone où l'on n'est pas, ce qui
   * corrige un « dans ce lieu » devenu faux pendant que les zones étaient
   * effacées. Jamais d'entrée signalée à l'inscription : pas de fausse
   * arrivée si l'on est déjà sur place.
   */
  @SuppressLint("MissingPermission")
  fun register(
    context: Context,
    places: List<Place>,
    checkOutside: Boolean,
    onSuccess: () -> Unit,
    onError: (String) -> Unit
  ) {
    missingPermissionMessage(context)?.let {
      onError(it)
      return
    }
    if (places.isEmpty()) {
      onSuccess()
      return
    }
    val builder = GeofencingRequest.Builder()
      .setInitialTrigger(if (checkOutside) GeofencingRequest.INITIAL_TRIGGER_EXIT else 0)
    for (place in places) {
      builder.addGeofence(
        Geofence.Builder()
          .setRequestId(place.id)
          .setCircularRegion(place.lat, place.lng, place.radiusM)
          .setExpirationDuration(Geofence.NEVER_EXPIRE)
          .setTransitionTypes(Geofence.GEOFENCE_TRANSITION_ENTER or Geofence.GEOFENCE_TRANSITION_EXIT)
          .build()
      )
    }
    try {
      LocationServices.getGeofencingClient(context.applicationContext)
        .addGeofences(builder.build(), pendingIntent(context))
        .addOnSuccessListener { onSuccess() }
        .addOnFailureListener { e -> onError(errorMessage(e, "d'enregistrer le lieu")) }
    } catch (e: SecurityException) {
      onError("Autorisation de position refusée par Android.")
    }
  }

  fun unregister(context: Context, ids: List<String>, onSuccess: () -> Unit, onError: (String) -> Unit) {
    LocationServices.getGeofencingClient(context.applicationContext)
      .removeGeofences(ids)
      .addOnSuccessListener { onSuccess() }
      .addOnFailureListener { e -> onError(errorMessage(e, "de retirer le lieu")) }
  }

  private fun errorMessage(e: Exception, action: String): String {
    val code = (e as? ApiException)?.statusCode
    return when (code) {
      GeofenceStatusCodes.GEOFENCE_NOT_AVAILABLE ->
        "Le géofencing est indisponible : activez la localisation (et la précision de la position Google) dans les réglages."
      GeofenceStatusCodes.GEOFENCE_TOO_MANY_GEOFENCES,
      GeofenceStatusCodes.GEOFENCE_TOO_MANY_PENDING_INTENTS ->
        "Android refuse d'enregistrer une géofence de plus."
      // GEOFENCE_INSUFFICIENT_LOCATION_PERMISSION
      1004 -> "Choisissez « Toujours autoriser » pour la position afin de détecter l'arrivée."
      // GEOFENCE_REQUEST_TOO_FREQUENT
      1005 -> "Trop de modifications d'affilée : réessayez dans quelques minutes."
      else -> "Impossible $action : ${e.localizedMessage ?: e.toString()}"
    }
  }
}
