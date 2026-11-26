package fr.eveetlucas.geofence

import android.Manifest
import android.annotation.SuppressLint
import android.app.Activity
import android.os.Build
import android.webkit.WebView
import androidx.appcompat.app.AppCompatActivity
import app.tauri.PermissionState
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.Permission
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSArray
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import com.google.android.gms.location.LocationServices
import com.google.android.gms.location.Priority
import com.google.android.gms.tasks.CancellationTokenSource
import org.unifiedpush.android.connector.INSTANCE_DEFAULT
import org.unifiedpush.android.connector.UnifiedPush
import org.unifiedpush.android.connector.data.ResolvedDistributor

private const val ALIAS_LOCATION = "location"
private const val ALIAS_BACKGROUND = "backgroundLocation"
private const val ALIAS_NOTIFICATIONS = "notifications"
private val ALL_ALIASES = listOf(ALIAS_LOCATION, ALIAS_BACKGROUND, ALIAS_NOTIFICATIONS)

@InvokeArg
class SetHomeArgs {
  var lat: Double = 0.0
  var lng: Double = 0.0
  var radiusM: Float = 0f
}

@InvokeArg
class RingArgs {
  var ring: Boolean = false
}

@InvokeArg
class IdsArgs {
  var ids: List<String>? = null
}

@InvokeArg
class PushConfigArgs {
  var key: String? = null
  var who: String? = null
  var device: String? = null
  var targets: List<String>? = null
}

@InvokeArg
class PermissionsArgs {
  var permissions: List<String>? = null
}

@TauriPlugin(
  permissions = [
    Permission(
      strings = [Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION],
      alias = ALIAS_LOCATION
    ),
    Permission(strings = [Manifest.permission.ACCESS_BACKGROUND_LOCATION], alias = ALIAS_BACKGROUND),
    Permission(strings = [Manifest.permission.POST_NOTIFICATIONS], alias = ALIAS_NOTIFICATIONS)
  ]
)
class GeofencePlugin(private val activity: Activity) : Plugin(activity) {

  companion object {
    /** Instance vivante (appli ouverte), pour prévenir le Rust d'une arrivée. */
    @Volatile
    private var instance: GeofencePlugin? = null

    /** Paquet de l'appli ntfy, préféré s'il y a plusieurs distributeurs sans choix par défaut. */
    private const val NTFY_PACKAGE = "io.heckel.ntfy"

    fun notifyArrival(arrival: PendingArrival) {
      val plugin = instance ?: return
      val payload = JSObject()
      payload.put("id", arrival.id)
      payload.put("at", arrival.at)
      plugin.trigger("arrival", payload)
    }

    fun notifyReceived(arrival: ArrivalMessage) {
      val plugin = instance ?: return
      plugin.trigger("received", receivedJson(arrival))
    }

    fun notifyPushEndpoint(endpoint: String) {
      val plugin = instance ?: return
      val payload = JSObject()
      payload.put("endpoint", endpoint)
      plugin.trigger("pushEndpoint", payload)
    }

    private fun receivedJson(arrival: ArrivalMessage): JSObject {
      val o = JSObject()
      o.put("id", arrival.id)
      o.put("who", arrival.who)
      o.put("device", arrival.device)
      o.put("at", arrival.at)
      return o
    }
  }

  override fun load(webView: WebView) {
    super.load(webView)
    instance = this
    ArrivalNotifier.createChannel(activity)
    // Filet de sécurité : la géofence peut avoir été effacée (localisation
    // coupée, données des services Google Play vidées…). La réenregistrer
    // à chaque lancement ne coûte rien.
    val home = GeofenceStore.home(activity)
    if (home != null && GeofenceRegistrar.canGeofence(activity)) {
      GeofenceRegistrar.register(
        activity,
        home,
        onSuccess = {},
        onError = { message -> android.util.Log.w(TAG, "Maison non réenregistrée : $message") }
      )
    }
  }

  override fun onDestroy(activity: AppCompatActivity) {
    if (instance === this) instance = null
    super.onDestroy(activity)
  }

  @Command
  fun setHome(invoke: Invoke) {
    val args = invoke.parseArgs(SetHomeArgs::class.java)
    val home = Home(args.lat, args.lng, args.radiusM)
    GeofenceRegistrar.register(
      activity,
      home,
      onSuccess = {
        GeofenceStore.saveHome(activity, home)
        invoke.resolve()
      },
      onError = { message -> invoke.reject(message) }
    )
  }

  @Command
  fun clearHome(invoke: Invoke) {
    GeofenceStore.clearHome(activity)
    GeofenceRegistrar.unregister(
      activity,
      onSuccess = { invoke.resolve() },
      onError = { message -> invoke.reject(message) }
    )
  }

  @Command
  @SuppressLint("MissingPermission")
  fun currentPosition(invoke: Invoke) {
    val fine = GeofenceRegistrar.hasPermission(activity, Manifest.permission.ACCESS_FINE_LOCATION)
    val coarse = GeofenceRegistrar.hasPermission(activity, Manifest.permission.ACCESS_COARSE_LOCATION)
    if (!fine && !coarse) {
      invoke.reject("Autorisez la position pour enregistrer l'emplacement de la maison.")
      return
    }
    try {
      LocationServices.getFusedLocationProviderClient(activity)
        .getCurrentLocation(Priority.PRIORITY_HIGH_ACCURACY, CancellationTokenSource().token)
        .addOnSuccessListener { location ->
          if (location == null) {
            invoke.reject("Position indisponible : vérifiez que la localisation est activée.")
          } else {
            val result = JSObject()
            result.put("lat", location.latitude)
            result.put("lng", location.longitude)
            result.put("accuracyM", location.accuracy.toDouble())
            invoke.resolve(result)
          }
        }
        .addOnFailureListener { e ->
          invoke.reject("Impossible d'obtenir la position : ${e.localizedMessage ?: e.toString()}")
        }
    } catch (e: SecurityException) {
      invoke.reject("Autorisation de position refusée par Android.")
    }
  }

  @Command
  fun setRingOnArrival(invoke: Invoke) {
    val args = invoke.parseArgs(RingArgs::class.java)
    GeofenceStore.setRingOnArrival(activity, args.ring)
    invoke.resolve()
  }

  @Command
  fun ackArrivals(invoke: Invoke) {
    val args = invoke.parseArgs(IdsArgs::class.java)
    GeofenceStore.ackArrivals(activity, args.ids.orEmpty().toSet())
    invoke.resolve()
  }

  @Command
  fun peekArrivals(invoke: Invoke) {
    val arrivals = JSArray()
    for (a in GeofenceStore.peekArrivals(activity)) {
      val o = JSObject()
      o.put("id", a.id)
      o.put("at", a.at)
      arrivals.put(o)
    }
    val result = JSObject()
    result.put("arrivals", arrivals)
    invoke.resolve(result)
  }

  /** Réglages du réveil par ntfy (réécrits à chaque appel ; clé vide : pas d'envoi). */
  @Command
  fun setPushConfig(invoke: Invoke) {
    val args = invoke.parseArgs(PushConfigArgs::class.java)
    val key = args.key.orEmpty().trim()
    if (key.isNotEmpty() && PushCrypto.decodeKey(key) == null) {
      invoke.reject("Clé de chiffrement invalide : 32 octets en base64 attendus.")
      return
    }
    val config = PushConfig(
      key = key,
      who = args.who.orEmpty().trim(),
      device = args.device.orEmpty().trim(),
      targets = PushSender.validTargets(args.targets.orEmpty())
    )
    GeofenceStore.savePushConfig(activity, config)
    invoke.resolve()
  }

  /**
   * S'enregistre auprès du distributeur UnifiedPush (l'appli ntfy). Sans
   * distributeur installé : `available: false`, sans erreur. L'adresse arrive
   * plus tard par l'évènement `pushEndpoint` ; celle déjà connue est renvoyée.
   */
  @Command
  fun registerPush(invoke: Invoke) {
    try {
      val distributors = UnifiedPush.getDistributors(activity).filter { it != activity.packageName }
      if (distributors.isEmpty()) {
        invoke.resolve(pushStatus(false))
        return
      }
      if (UnifiedPush.getSavedDistributor(activity) == null) {
        val chosen = when (val resolved = UnifiedPush.resolveDefaultDistributor(activity)) {
          is ResolvedDistributor.Found -> resolved.packageName
          // Plusieurs distributeurs sans choix par défaut : pas de fenêtre
          // système à ce stade, on prend ntfy s'il est là.
          else -> if (NTFY_PACKAGE in distributors) NTFY_PACKAGE else distributors.first()
        }
        UnifiedPush.saveDistributor(activity, chosen)
      }
      UnifiedPush.register(activity, INSTANCE_DEFAULT, "Eve & Lucas : arrivées à la maison", null)
      invoke.resolve(pushStatus(true))
    } catch (e: Exception) {
      invoke.reject("Enregistrement auprès de ntfy impossible : ${e.localizedMessage ?: e.toString()}")
    }
  }

  private fun pushStatus(available: Boolean): JSObject {
    val result = JSObject()
    result.put("available", available)
    GeofenceStore.pushEndpoint(activity)?.let { if (available) result.put("endpoint", it) }
    return result
  }

  @Command
  fun peekReceived(invoke: Invoke) {
    val received = JSArray()
    for (a in GeofenceStore.peekReceived(activity)) received.put(receivedJson(a))
    val result = JSObject()
    result.put("received", received)
    invoke.resolve(result)
  }

  @Command
  fun ackReceived(invoke: Invoke) {
    val args = invoke.parseArgs(IdsArgs::class.java)
    GeofenceStore.ackReceived(activity, args.ids.orEmpty().toSet())
    invoke.resolve()
  }

  /** L'appli a déjà fait sonner ces arrivées : ntfy ne les fera pas sonner une deuxième fois. */
  @Command
  fun markRung(invoke: Invoke) {
    val args = invoke.parseArgs(IdsArgs::class.java)
    GeofenceStore.markRung(activity, args.ids.orEmpty())
    invoke.resolve()
  }

  /**
   * Avant Android 10, la position au premier plan suffit pour l'arrière-plan ;
   * avant Android 13, les notifications n'ont pas besoin d'autorisation.
   */
  override fun getPermissionStates(): Map<String, PermissionState> {
    val states = super.getPermissionStates().toMutableMap()
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
      states[ALIAS_BACKGROUND] = states[ALIAS_LOCATION] ?: PermissionState.PROMPT
    }
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) {
      states[ALIAS_NOTIFICATIONS] = PermissionState.GRANTED
    }
    return states
  }

  /**
   * Demande les autorisations voulues (par défaut : position et notifications,
   * jamais l'arrière-plan sans le demander explicitement). Répond avec l'état
   * de toutes les autorisations, comme `checkPermissions`.
   */
  @Command
  override fun requestPermissions(invoke: Invoke) {
    val args = invoke.parseArgs(PermissionsArgs::class.java)
    val asked = args.permissions ?: listOf(ALIAS_LOCATION, ALIAS_NOTIFICATIONS)
    val unknown = asked.filter { it !in ALL_ALIASES }
    if (unknown.isNotEmpty()) {
      invoke.reject("Autorisation inconnue : ${unknown.joinToString(", ")}")
      return
    }
    val states = getPermissionStates()
    val toRequest = asked.distinct().filter { alias ->
      states[alias] != PermissionState.GRANTED && when (alias) {
        ALIAS_BACKGROUND -> Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q
        ALIAS_NOTIFICATIONS -> Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU
        else -> true
      }
    }
    // Android 10+ ignore une demande d'arrière-plan faite avant (ou en même
    // temps que) celle de la position au premier plan.
    if (ALIAS_BACKGROUND in toRequest && states[ALIAS_LOCATION] != PermissionState.GRANTED) {
      invoke.reject(
        "Autorisez d'abord la position (au premier plan), puis demandez la position en arrière-plan séparément."
      )
      return
    }
    if (toRequest.isEmpty()) {
      checkPermissions(invoke)
      return
    }
    requestPermissionForAliases(toRequest.toTypedArray(), invoke, "checkPermissions")
  }
}
