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

/** Enregistrement de la géofence « maison » auprès des services Google Play. */
object GeofenceRegistrar {
  const val GEOFENCE_ID = "maison"
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
      return "Autorisez d'abord la position précise : elle sert à savoir quand vous arrivez à la maison."
    }
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q &&
      !hasPermission(context, Manifest.permission.ACCESS_BACKGROUND_LOCATION)
    ) {
      return "Choisissez « Toujours autoriser » pour la position : sans cela, l'arrivée n'est pas détectée quand l'appli est fermée."
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

  /** Enregistre (ou remplace) la géofence. Les rappels arrivent sur le fil principal. */
  @SuppressLint("MissingPermission")
  fun register(context: Context, home: Home, onSuccess: () -> Unit, onError: (String) -> Unit) {
    missingPermissionMessage(context)?.let {
      onError(it)
      return
    }
    val geofence = Geofence.Builder()
      .setRequestId(GEOFENCE_ID)
      .setCircularRegion(home.lat, home.lng, home.radiusM)
      .setExpirationDuration(Geofence.NEVER_EXPIRE)
      .setTransitionTypes(Geofence.GEOFENCE_TRANSITION_ENTER)
      .build()
    // Déclencheur initial 0 : pas d'« arrivée » si l'on est déjà à la maison.
    val request = GeofencingRequest.Builder()
      .setInitialTrigger(0)
      .addGeofence(geofence)
      .build()
    try {
      LocationServices.getGeofencingClient(context.applicationContext)
        .addGeofences(request, pendingIntent(context))
        .addOnSuccessListener { onSuccess() }
        .addOnFailureListener { e -> onError(errorMessage(e, "d'enregistrer la maison")) }
    } catch (e: SecurityException) {
      onError("Autorisation de position refusée par Android.")
    }
  }

  fun unregister(context: Context, onSuccess: () -> Unit, onError: (String) -> Unit) {
    LocationServices.getGeofencingClient(context.applicationContext)
      .removeGeofences(listOf(GEOFENCE_ID))
      .addOnSuccessListener { onSuccess() }
      .addOnFailureListener { e -> onError(errorMessage(e, "d'effacer la maison")) }
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
