package fr.eveetlucas.geofence

import android.content.Context
import android.content.SharedPreferences
import android.util.Log
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID

/**
 * Un lieu enregistré sur ce téléphone (maison, université, travail…) :
 * une zone de géorepérage. [announce] : y entrer fait sonner les autres.
 */
data class Place(
  val id: String,
  val name: String,
  val kind: String,
  val lat: Double,
  val lng: Double,
  val radiusM: Float,
  val announce: Boolean
)

/** Types de lieux (les mêmes que `couple_core::presence::PLACE_KINDS`). */
object PlaceKinds {
  val ALL = listOf("maison", "etudes", "travail", "autre")

  fun emoji(kind: String): String = when (kind) {
    "maison", "" -> "🏠"
    "etudes" -> "🎓"
    "travail" -> "💼"
    else -> "📍"
  }
}

/**
 * Un évènement de ce téléphone, pas encore enregistré par l'appli :
 * `arrival` (entrée dans un lieu annoncé, les autres sonnent) ou `presence`
 * (entrée ailleurs, ou sortie de tous les lieux : rien ne sonne).
 */
data class PendingArrival(
  val id: String,
  val at: Long,
  val kind: String = KIND_ARRIVAL,
  val inside: Boolean = true,
  val place: String = "",
  val placeKind: String = ""
)

const val KIND_ARRIVAL = "arrival"
const val KIND_PRESENCE = "presence"

/**
 * Réglages du réveil par ntfy, fournis par l'appli : clé commune (base64),
 * qui porte ce téléphone, son identifiant iroh, les adresses ntfy des
 * autres appareils et les types de nouveautés à afficher ici.
 */
data class PushConfig(
  val key: String,
  val who: String,
  val device: String,
  val targets: List<String>,
  val notify: List<String> = EventPayload.KINDS.toList(),
  val owner: String = "",
  val devices: List<String> = emptyList()
)

/**
 * Préférences du plugin : les lieux (pour les réenregistrer après un
 * redémarrage) et ceux où se trouve le téléphone, l'option « sonner à
 * l'arrivée », les arrivées et présences pas encore lues par l'appli, et pour le réveil par ntfy : la configuration d'envoi,
 * l'adresse UnifiedPush de ce téléphone, les arrivées reçues et celles qui
 * ont déjà sonné.
 *
 * Partagées entre le plugin (fil principal) et les récepteurs : chaque accès
 * est synchronisé et écrit avec `commit()` pour ne rien perdre si Android
 * arrête le processus juste après.
 */
object GeofenceStore {
  private const val PREFS = "fr.eveetlucas.geofence"
  private const val KEY_PLACES = "places"
  private const val KEY_INSIDE = "inside_ids"
  private const val KEY_HAS_HOME = "has_home"
  private const val KEY_LAT = "lat_bits"
  private const val KEY_LNG = "lng_bits"
  private const val KEY_RADIUS = "radius_m"
  private const val KEY_RING = "ring_on_arrival"
  private const val KEY_ARRIVALS = "arrivals"
  private const val KEY_LAST_ARRIVAL = "last_arrival_at"
  private const val KEY_PUSH_KEY = "push_key"
  private const val KEY_PUSH_WHO = "push_who"
  private const val KEY_PUSH_DEVICE = "push_device"
  private const val KEY_PUSH_TARGETS = "push_targets"
  private const val KEY_PUSH_ENDPOINT = "push_endpoint"
  private const val KEY_RUNG = "rung_ids"
  private const val KEY_RECEIVED = "received"
  private const val KEY_PUSH_NOTIFY = "push_notify"
  private const val KEY_PUSH_OWNER = "push_owner"
  private const val KEY_PUSH_DEVICES = "push_devices"
  private const val KEY_SHOWN = "shown_event_ids"

  /** Nouveautés « déjà affichées » gardées (les plus récentes). */
  private const val MAX_SHOWN = 100

  /** Anti-rebond : deux entrées à moins de 5 min ne font qu'une arrivée. */
  const val DEBOUNCE_MS = 5 * 60 * 1000L

  /** Identifiants « déjà sonnés » gardés (les plus récents). */
  private const val MAX_RUNG = 50

  /** Nombre maximal de lieux (Android en accepte 100 par appli). */
  const val MAX_PLACES = 10

  /** Au-delà, les arrivées les plus anciennes sont oubliées. */
  private const val MAX_ARRIVALS = 100

  private fun prefs(context: Context): SharedPreferences =
    context.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

  /**
   * Lieux enregistrés. L'ancienne maison unique (avant les lieux nommés)
   * devient le lieu « Maison », annoncé, sous le même identifiant que son
   * ancienne géofence.
   */
  @Synchronized
  fun places(context: Context): List<Place> {
    val p = prefs(context)
    if (!p.contains(KEY_PLACES) && p.getBoolean(KEY_HAS_HOME, false)) {
      val home = Place(
        id = GeofenceRegistrar.LEGACY_ID,
        name = "Maison",
        kind = "maison",
        lat = Double.fromBits(p.getLong(KEY_LAT, 0L)),
        lng = Double.fromBits(p.getLong(KEY_LNG, 0L)),
        radiusM = p.getFloat(KEY_RADIUS, 0f),
        announce = true
      )
      p.edit()
        .putString(KEY_PLACES, placesJson(listOf(home)))
        .remove(KEY_HAS_HOME)
        .remove(KEY_LAT)
        .remove(KEY_LNG)
        .remove(KEY_RADIUS)
        .commit()
    }
    return readPlaces(p)
  }

  @Synchronized
  fun addPlace(context: Context, place: Place) {
    val list = places(context).filter { it.id != place.id } + place
    prefs(context).edit().putString(KEY_PLACES, placesJson(list)).commit()
  }

  /** Renomme un lieu ; `false` s'il n'existe pas. */
  @Synchronized
  fun renamePlace(context: Context, id: String, name: String): Boolean {
    val list = places(context)
    if (list.none { it.id == id }) return false
    val renamed = list.map { if (it.id == id) it.copy(name = name) else it }
    prefs(context).edit().putString(KEY_PLACES, placesJson(renamed)).commit()
    return true
  }

  @Synchronized
  fun removePlace(context: Context, id: String) {
    val p = prefs(context)
    val rest = places(context).filter { it.id != id }
    val inside = readStrings(p, KEY_INSIDE).filter { it != id }
    p.edit()
      .putString(KEY_PLACES, placesJson(rest))
      .putString(KEY_INSIDE, JSONArray(inside).toString())
      .commit()
  }

  /** Identifiants des lieux où se trouve le téléphone, le plus récent en dernier. */
  @Synchronized
  fun insideIds(context: Context): List<String> {
    val known = places(context).map { it.id }.toSet()
    return readStrings(prefs(context), KEY_INSIDE).filter { it in known }
  }

  private fun readPlaces(p: SharedPreferences): List<Place> {
    val list = mutableListOf<Place>()
    val raw = p.getString(KEY_PLACES, null) ?: return list
    try {
      val json = JSONArray(raw)
      for (i in 0 until json.length()) {
        val o = json.optJSONObject(i) ?: continue
        val id = o.opt("id") as? String ?: continue
        val name = o.opt("name") as? String ?: continue
        val radius = o.optDouble("radiusM", 0.0)
        if (id.isEmpty() || radius <= 0.0) continue
        list.add(
          Place(
            id = id,
            name = name,
            kind = (o.opt("kind") as? String)?.takeIf { it in PlaceKinds.ALL } ?: "autre",
            lat = o.optDouble("lat", 0.0),
            lng = o.optDouble("lng", 0.0),
            radiusM = radius.toFloat(),
            announce = o.optBoolean("announce", false)
          )
        )
      }
    } catch (e: Exception) {
      Log.w(TAG, "Liste des lieux illisible, ignorée", e)
    }
    return list
  }

  private fun placesJson(places: List<Place>): String {
    val json = JSONArray()
    for (pl in places) {
      json.put(
        JSONObject()
          .put("id", pl.id)
          .put("name", pl.name)
          .put("kind", pl.kind)
          .put("lat", pl.lat)
          .put("lng", pl.lng)
          .put("radiusM", pl.radiusM.toDouble())
          .put("announce", pl.announce)
      )
    }
    return json.toString()
  }

  @Synchronized
  fun setRingOnArrival(context: Context, ring: Boolean) {
    prefs(context).edit().putBoolean(KEY_RING, ring).commit()
  }

  @Synchronized
  fun ringOnArrival(context: Context): Boolean = prefs(context).getBoolean(KEY_RING, false)

  /**
   * Note des entrées ([entered]) et des sorties ([exited]) de lieux, et
   * renvoie l'évènement à annoncer si le lieu actuel (le dernier où l'on est
   * entré et qu'on n'a pas quitté) change, sinon `null`.
   *
   * Passer d'« ailleurs » à un lieu annoncé est une arrivée (les autres
   * sonnent), sauf si [allowArrival] est faux (lieu tout juste ajouté) ou
   * moins de 5 minutes après la précédente arrivée (même anti-rebond que le
   * cœur) ; tout autre changement est une simple présence. L'identifiant est
   * créé ici, pour que l'évènement envoyé par ntfy et celui que l'appli
   * enregistrera plus tard dans le document partagé soient le même.
   */
  @Synchronized
  fun recordTransition(
    context: Context,
    at: Long,
    entered: List<String>,
    exited: List<String>,
    allowArrival: Boolean
  ): PendingArrival? {
    val p = prefs(context)
    val byId = places(context).associateBy { it.id }
    val inside = readStrings(p, KEY_INSIDE).filter { it in byId }.toMutableList()
    val before = inside.lastOrNull()
    for (id in exited) inside.remove(id)
    for (id in entered) {
      if (id !in byId) continue
      inside.remove(id)
      inside.add(id)
    }
    val after = inside.lastOrNull()
    val edit = p.edit().putString(KEY_INSIDE, JSONArray(inside).toString())
    if (before == after) {
      edit.commit()
      return null
    }
    val arrivals = readArrivals(p)
    val event = if (after != null) {
      val place = byId.getValue(after)
      var last = p.getLong(KEY_LAST_ARRIVAL, Long.MIN_VALUE)
      for (a in arrivals) if (a.kind == KIND_ARRIVAL && a.at > last) last = a.at
      val bounce = last != Long.MIN_VALUE && at >= last && at - last < DEBOUNCE_MS
      val arrival = allowArrival && before == null && place.announce && !bounce
      if (arrival) edit.putLong(KEY_LAST_ARRIVAL, at)
      PendingArrival(
        UUID.randomUUID().toString(),
        at,
        if (arrival) KIND_ARRIVAL else KIND_PRESENCE,
        true,
        place.name,
        place.kind
      )
    } else {
      val left = byId[before]
      PendingArrival(
        UUID.randomUUID().toString(),
        at,
        KIND_PRESENCE,
        false,
        left?.name.orEmpty(),
        left?.kind.orEmpty()
      )
    }
    arrivals.add(event)
    while (arrivals.size > MAX_ARRIVALS) arrivals.removeAt(0)
    edit.putString(KEY_ARRIVALS, arrivalsJson(arrivals)).commit()
    return event
  }

  /**
   * Note à nouveau le lieu actuel (renommé), s'il s'agit de [id] : simple
   * présence, sans sonnerie. `null` si l'on n'y est pas.
   */
  @Synchronized
  fun recordCurrent(context: Context, at: Long, id: String): PendingArrival? {
    val p = prefs(context)
    if (insideIds(context).lastOrNull() != id) return null
    val place = places(context).firstOrNull { it.id == id } ?: return null
    val event = PendingArrival(UUID.randomUUID().toString(), at, KIND_PRESENCE, true, place.name, place.kind)
    val arrivals = readArrivals(p)
    arrivals.add(event)
    while (arrivals.size > MAX_ARRIVALS) arrivals.removeAt(0)
    p.edit().putString(KEY_ARRIVALS, arrivalsJson(arrivals)).commit()
    return event
  }

  /** Renvoie les arrivées enregistrées, sans les effacer. */
  @Synchronized
  fun peekArrivals(context: Context): List<PendingArrival> = readArrivals(prefs(context))

  /**
   * Efface les arrivées dont l'appli confirme l'enregistrement : une arrivée
   * survenue entre-temps est gardée.
   */
  @Synchronized
  fun ackArrivals(context: Context, ids: Collection<String>) {
    val p = prefs(context)
    val rest = readArrivals(p).filter { it.id !in ids }
    p.edit().putString(KEY_ARRIVALS, arrivalsJson(rest)).commit()
  }

  /**
   * Une liste illisible est ignorée, jamais bloquante. L'ancien format (liste
   * de dates) est converti une fois pour toutes, avec de nouveaux
   * identifiants : il est réécrit tout de suite pour que `peek` et `ack`
   * voient les mêmes.
   */
  private fun readArrivals(p: SharedPreferences): MutableList<PendingArrival> {
    val list = mutableListOf<PendingArrival>()
    val raw = p.getString(KEY_ARRIVALS, null) ?: return list
    var migrated = false
    try {
      val json = JSONArray(raw)
      for (i in 0 until json.length()) {
        val item = json.opt(i)
        if (item is JSONObject) {
          val id = item.opt("id") as? String
          val at = item.optLong("at", -1L)
          if (id != null && id.isNotEmpty() && at >= 0L) {
            list.add(
              PendingArrival(
                id,
                at,
                if (item.opt("kind") == KIND_PRESENCE) KIND_PRESENCE else KIND_ARRIVAL,
                item.optBoolean("inside", true),
                item.opt("place") as? String ?: "",
                item.opt("placeKind") as? String ?: ""
              )
            )
          }
        } else if (item is Number) {
          val at = item.toLong()
          if (at >= 0L) list.add(PendingArrival(UUID.randomUUID().toString(), at))
          migrated = true
        }
      }
    } catch (e: Exception) {
      Log.w(TAG, "Liste des arrivées illisible, ignorée", e)
    }
    if (migrated) p.edit().putString(KEY_ARRIVALS, arrivalsJson(list)).commit()
    return list
  }

  private fun arrivalsJson(arrivals: List<PendingArrival>): String {
    val json = JSONArray()
    for (a in arrivals) json.put(pendingJson(a))
    return json.toString()
  }

  fun pendingJson(a: PendingArrival): JSONObject =
    JSONObject()
      .put("id", a.id)
      .put("at", a.at)
      .put("kind", a.kind)
      .put("inside", a.inside)
      .put("place", a.place)
      .put("placeKind", a.placeKind)

  // --- Réveil par ntfy ---------------------------------------------------

  /** Réécrit toute la configuration d'envoi (clé vide : pas d'envoi). */
  @Synchronized
  fun savePushConfig(context: Context, config: PushConfig) {
    val targets = JSONArray()
    for (t in config.targets) targets.put(t)
    prefs(context).edit()
      .putString(KEY_PUSH_KEY, config.key)
      .putString(KEY_PUSH_WHO, config.who)
      .putString(KEY_PUSH_DEVICE, config.device)
      .putString(KEY_PUSH_TARGETS, targets.toString())
      .putString(KEY_PUSH_NOTIFY, JSONArray(config.notify).toString())
      .putString(KEY_PUSH_OWNER, config.owner)
      .putString(KEY_PUSH_DEVICES, JSONArray(config.devices).toString())
      .commit()
  }

  @Synchronized
  fun pushConfig(context: Context): PushConfig {
    val p = prefs(context)
    val targets = mutableListOf<String>()
    try {
      val json = JSONArray(p.getString(KEY_PUSH_TARGETS, null) ?: "[]")
      for (i in 0 until json.length()) (json.opt(i) as? String)?.let { targets.add(it) }
    } catch (e: Exception) {
      Log.w(TAG, "Liste des adresses ntfy illisible, ignorée", e)
    }
    return PushConfig(
      key = p.getString(KEY_PUSH_KEY, null) ?: "",
      who = p.getString(KEY_PUSH_WHO, null) ?: "",
      device = p.getString(KEY_PUSH_DEVICE, null) ?: "",
      targets = targets,
      // Jamais reçu (appli pas encore rouverte depuis la mise à jour) : tout
      // est activé, comme dans l'appli.
      notify = if (p.contains(KEY_PUSH_NOTIFY)) readStrings(p, KEY_PUSH_NOTIFY) else EventPayload.KINDS.toList(),
      owner = p.getString(KEY_PUSH_OWNER, null) ?: "",
      devices = readStrings(p, KEY_PUSH_DEVICES)
    )
  }

  /** Adresse donnée par le distributeur UnifiedPush (`null` : aucune ou plus valable). */
  @Synchronized
  fun setPushEndpoint(context: Context, endpoint: String?) {
    val edit = prefs(context).edit()
    if (endpoint == null) edit.remove(KEY_PUSH_ENDPOINT) else edit.putString(KEY_PUSH_ENDPOINT, endpoint)
    edit.commit()
  }

  @Synchronized
  fun pushEndpoint(context: Context): String? = prefs(context).getString(KEY_PUSH_ENDPOINT, null)

  /** L'appli (ou ce module) a déjà fait sonner ces arrivées. */
  @Synchronized
  fun markRung(context: Context, ids: Collection<String>) {
    val p = prefs(context)
    val rung = readStrings(p, KEY_RUNG)
    for (id in ids) {
      if (id.isEmpty()) continue
      rung.remove(id)
      rung.add(id)
    }
    while (rung.size > MAX_RUNG) rung.removeAt(0)
    p.edit().putString(KEY_RUNG, JSONArray(rung).toString()).commit()
  }

  /**
   * Note que l'arrivée [id] sonne ici. Renvoie `false` si elle a déjà sonné
   * (vérification et écriture d'un seul tenant : jamais deux sonneries).
   */
  @Synchronized
  fun claimRing(context: Context, id: String): Boolean {
    if (readStrings(prefs(context), KEY_RUNG).contains(id)) return false
    markRung(context, listOf(id))
    return true
  }

  /**
   * Note que la nouveauté [id] est affichée ici. Renvoie `false` si elle
   * l'était déjà (même message reçu deux fois).
   */
  @Synchronized
  fun claimEvent(context: Context, id: String): Boolean {
    val p = prefs(context)
    val shown = readStrings(p, KEY_SHOWN)
    if (shown.contains(id)) return false
    shown.add(id)
    while (shown.size > MAX_SHOWN) shown.removeAt(0)
    p.edit().putString(KEY_SHOWN, JSONArray(shown).toString()).commit()
    return true
  }

  /** Arrivées des autres reçues par ntfy, en attendant que l'appli les lise. */
  @Synchronized
  fun addReceived(context: Context, arrival: ArrivalMessage): Boolean {
    val p = prefs(context)
    val list = readReceived(p)
    if (list.any { it.id == arrival.id }) return false
    list.add(arrival)
    while (list.size > MAX_ARRIVALS) list.removeAt(0)
    p.edit().putString(KEY_RECEIVED, receivedJson(list)).commit()
    return true
  }

  @Synchronized
  fun peekReceived(context: Context): List<ArrivalMessage> = readReceived(prefs(context))

  @Synchronized
  fun ackReceived(context: Context, ids: Collection<String>) {
    val p = prefs(context)
    val rest = readReceived(p).filter { it.id !in ids }
    p.edit().putString(KEY_RECEIVED, receivedJson(rest)).commit()
  }

  private fun readReceived(p: SharedPreferences): MutableList<ArrivalMessage> {
    val list = mutableListOf<ArrivalMessage>()
    val raw = p.getString(KEY_RECEIVED, null) ?: return list
    try {
      val json = JSONArray(raw)
      for (i in 0 until json.length()) {
        val o = json.optJSONObject(i) ?: continue
        val id = o.opt("id") as? String ?: continue
        val who = o.opt("who") as? String ?: continue
        val device = o.opt("device") as? String ?: continue
        val at = o.optLong("at", -1L)
        if (at >= 0L) {
          list.add(
            ArrivalMessage(
              id,
              who,
              device,
              at,
              if (o.opt("kind") == KIND_PRESENCE) KIND_PRESENCE else KIND_ARRIVAL,
              o.optBoolean("inside", true),
              o.opt("place") as? String ?: "",
              o.opt("placeKind") as? String ?: ""
            )
          )
        }
      }
    } catch (e: Exception) {
      Log.w(TAG, "Liste des arrivées reçues illisible, ignorée", e)
    }
    return list
  }

  private fun receivedJson(list: List<ArrivalMessage>): String {
    val json = JSONArray()
    for (a in list) {
      json.put(
        JSONObject()
          .put("id", a.id)
          .put("who", a.who)
          .put("device", a.device)
          .put("at", a.at)
          .put("kind", a.kind)
          .put("inside", a.inside)
          .put("place", a.place)
          .put("placeKind", a.placeKind)
      )
    }
    return json.toString()
  }

  private fun readStrings(p: SharedPreferences, key: String): MutableList<String> {
    val list = mutableListOf<String>()
    val raw = p.getString(key, null) ?: return list
    try {
      val json = JSONArray(raw)
      for (i in 0 until json.length()) (json.opt(i) as? String)?.let { list.add(it) }
    } catch (e: Exception) {
      Log.w(TAG, "Liste illisible ($key), ignorée", e)
    }
    return list
  }
}

internal const val TAG = "EveLucasGeofence"
