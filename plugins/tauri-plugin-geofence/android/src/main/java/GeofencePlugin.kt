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
import java.util.UUID

private const val ALIAS_LOCATION = "location"
private const val ALIAS_BACKGROUND = "backgroundLocation"
private const val ALIAS_NOTIFICATIONS = "notifications"
private val ALL_ALIASES = listOf(ALIAS_LOCATION, ALIAS_BACKGROUND, ALIAS_NOTIFICATIONS)

@InvokeArg
class AddPlaceArgs {
  var name: String? = null
  var kind: String? = null
  var announce: Boolean = false
  var lat: Double = 0.0
  var lng: Double = 0.0
  var radiusM: Float = 0f
}

@InvokeArg
class RenamePlaceArgs {
  var id: String? = null
  var name: String? = null
}

@InvokeArg
class PlaceIdArgs {
  var id: String? = null
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
  var notify: List<String>? = null
  var owner: String? = null
  var devices: List<String>? = null
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
      payload.put("kind", arrival.kind)
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
      o.put("kind", arrival.kind)
      o.put("inside", arrival.inside)
      o.put("place", arrival.place)
      o.put("placeKind", arrival.placeKind)
      return o
    }

    private fun placeJson(place: Place, inside: Boolean): JSObject {
      val o = JSObject()
      o.put("id", place.id)
      o.put("name", place.name)
      o.put("kind", place.kind)
      o.put("lat", place.lat)
      o.put("lng", place.lng)
      o.put("radiusM", place.radiusM.toDouble())
      o.put("announce", place.announce)
      o.put("inside", inside)
      return o
    }
  }

  override fun load(webView: WebView) {
    super.load(webView)
    instance = this
    ArrivalNotifier.createChannel(activity)
    // Filet de sécurité : les zones peuvent avoir été effacées (localisation
    // coupée, données des services Google Play vidées…). Les réenregistrer
    // à chaque lancement ne coûte rien.
    val places = GeofenceStore.places(activity)
    if (places.isNotEmpty() && GeofenceRegistrar.canGeofence(activity)) {
      GeofenceRegistrar.register(
        activity,
        places,
        checkOutside = true,
        onSuccess = {},
        onError = { message -> android.util.Log.w(TAG, "Lieux non réenregistrés : $message") }
      )
    }
  }

  override fun onDestroy(activity: AppCompatActivity) {
    if (instance === this) instance = null
    super.onDestroy(activity)
  }

  /**
   * Ajoute un lieu là où se trouve le téléphone : il y est donc entré (simple
   * présence, sans sonnerie), et les autres l'apprennent.
   */
  @Command
  fun addPlace(invoke: Invoke) {
    val args = invoke.parseArgs(AddPlaceArgs::class.java)
    val name = args.name.orEmpty().trim()
    val kind = args.kind.orEmpty()
    if (name.isEmpty() || kind !in PlaceKinds.ALL || args.radiusM <= 0f) {
      invoke.reject("Lieu invalide.")
      return
    }
    if (GeofenceStore.places(activity).size >= GeofenceStore.MAX_PLACES) {
      invoke.reject("${GeofenceStore.MAX_PLACES} lieux au plus : retires-en un d'abord.")
      return
    }
    val place = Place(
      UUID.randomUUID().toString(),
      name,
      kind,
      args.lat,
      args.lng,
      args.radiusM,
      args.announce
    )
    GeofenceRegistrar.register(
      activity,
      listOf(place),
      checkOutside = false,
      onSuccess = {
        GeofenceStore.addPlace(activity, place)
        GeofenceStore.recordTransition(
          activity,
          System.currentTimeMillis(),
          listOf(place.id),
          emptyList(),
          allowArrival = false
        )?.let { announce(it) }
        invoke.resolve(placeJson(place, true))
      },
      onError = { message -> invoke.reject(message) }
    )
  }

  @Command
  fun renamePlace(invoke: Invoke) {
    val args = invoke.parseArgs(RenamePlaceArgs::class.java)
    val name = args.name.orEmpty().trim()
    if (name.isEmpty() || !GeofenceStore.renamePlace(activity, args.id.orEmpty(), name)) {
      invoke.reject("Ce lieu n'existe plus.")
      return
    }
    invoke.resolve()
  }

  /** Retire un lieu ; si l'on y était, les autres apprennent qu'on n'y est plus. */
  @Command
  fun removePlace(invoke: Invoke) {
    val id = invoke.parseArgs(PlaceIdArgs::class.java).id.orEmpty()
    GeofenceRegistrar.unregister(
      activity,
      listOf(id),
      onSuccess = {
        GeofenceStore.recordTransition(
          activity,
          System.currentTimeMillis(),
          emptyList(),
          listOf(id),
          allowArrival = false
        )?.let { announce(it) }
        GeofenceStore.removePlace(activity, id)
        invoke.resolve()
      },
      onError = { message -> invoke.reject(message) }
    )
  }

  @Command
  fun listPlaces(invoke: Invoke) {
    val inside = GeofenceStore.insideIds(activity).toSet()
    val places = JSArray()
    for (p in GeofenceStore.places(activity)) places.put(placeJson(p, p.id in inside))
    val result = JSObject()
    result.put("places", places)
    invoke.resolve(result)
  }

  /** Changement de lieu décidé dans l'appli : l'appli le lit, les autres le reçoivent. */
  private fun announce(event: PendingArrival) {
    notifyArrival(event)
    PresenceAnnouncer.push(activity, event) {}
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
      o.put("kind", a.kind)
      o.put("inside", a.inside)
      o.put("place", a.place)
      o.put("placeKind", a.placeKind)
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
      targets = PushSender.validTargets(args.targets.orEmpty()),
      notify = args.notify.orEmpty().filter { it in EventPayload.KINDS }.distinct(),
      owner = args.owner.orEmpty().trim(),
      devices = args.devices.orEmpty().map { it.trim() }.filter { it.isNotEmpty() }.distinct()
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
