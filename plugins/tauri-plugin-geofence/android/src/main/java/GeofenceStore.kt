package fr.eveetlucas.geofence

import android.content.Context
import android.content.SharedPreferences
import android.util.Log
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID

/** Coordonnées de la maison enregistrées. */
data class Home(val lat: Double, val lng: Double, val radiusM: Float)

/** Une arrivée de ce téléphone, pas encore enregistrée par l'appli. */
data class PendingArrival(val id: String, val at: Long)

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
 * Préférences du plugin : la maison (pour la réenregistrer après un
 * redémarrage), l'option « sonner à l'arrivée », les arrivées pas encore
 * lues par l'appli, et pour le réveil par ntfy : la configuration d'envoi,
 * l'adresse UnifiedPush de ce téléphone, les arrivées reçues et celles qui
 * ont déjà sonné.
 *
 * Partagées entre le plugin (fil principal) et les récepteurs : chaque accès
 * est synchronisé et écrit avec `commit()` pour ne rien perdre si Android
 * arrête le processus juste après.
 */
object GeofenceStore {
  private const val PREFS = "fr.eveetlucas.geofence"
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

  /** Au-delà, les arrivées les plus anciennes sont oubliées. */
  private const val MAX_ARRIVALS = 100

  private fun prefs(context: Context): SharedPreferences =
    context.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

  @Synchronized
  fun saveHome(context: Context, home: Home) {
    prefs(context).edit()
      .putBoolean(KEY_HAS_HOME, true)
      // Pas de `putDouble` : on garde les 64 bits exacts dans un Long.
      .putLong(KEY_LAT, home.lat.toRawBits())
      .putLong(KEY_LNG, home.lng.toRawBits())
      .putFloat(KEY_RADIUS, home.radiusM)
      .commit()
  }

  @Synchronized
  fun home(context: Context): Home? {
    val p = prefs(context)
    if (!p.getBoolean(KEY_HAS_HOME, false)) return null
    return Home(
      Double.fromBits(p.getLong(KEY_LAT, 0L)),
      Double.fromBits(p.getLong(KEY_LNG, 0L)),
      p.getFloat(KEY_RADIUS, 0f)
    )
  }

  @Synchronized
  fun clearHome(context: Context) {
    prefs(context).edit()
      .remove(KEY_HAS_HOME)
      .remove(KEY_LAT)
      .remove(KEY_LNG)
      .remove(KEY_RADIUS)
      .commit()
  }

  @Synchronized
  fun setRingOnArrival(context: Context, ring: Boolean) {
    prefs(context).edit().putBoolean(KEY_RING, ring).commit()
  }

  @Synchronized
  fun ringOnArrival(context: Context): Boolean = prefs(context).getBoolean(KEY_RING, false)

  /**
   * Note une entrée dans la zone, sauf si la précédente arrivée de ce
   * téléphone date de moins de 5 minutes (même anti-rebond que le cœur).
   * L'identifiant est créé ici, pour que l'arrivée envoyée par ntfy et celle
   * que l'appli enregistrera plus tard dans le document partagé soient la même.
   *
   * @return l'arrivée notée, ou `null` si elle est ignorée.
   */
  @Synchronized
  fun recordArrival(context: Context, at: Long): PendingArrival? {
    val p = prefs(context)
    val arrivals = readArrivals(p)
    var last = p.getLong(KEY_LAST_ARRIVAL, Long.MIN_VALUE)
    for (a in arrivals) if (a.at > last) last = a.at
    if (last != Long.MIN_VALUE && at >= last && at - last < DEBOUNCE_MS) return null
    val arrival = PendingArrival(UUID.randomUUID().toString(), at)
    arrivals.add(arrival)
    while (arrivals.size > MAX_ARRIVALS) arrivals.removeAt(0)
    p.edit()
      .putString(KEY_ARRIVALS, arrivalsJson(arrivals))
      .putLong(KEY_LAST_ARRIVAL, at)
      .commit()
    return arrival
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
          if (id != null && id.isNotEmpty() && at >= 0L) list.add(PendingArrival(id, at))
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
    for (a in arrivals) json.put(JSONObject().put("id", a.id).put("at", a.at))
    return json.toString()
  }

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
  fun addReceived(context: Context, arrival: ArrivalMessage) {
    val p = prefs(context)
    val list = readReceived(p)
    if (list.any { it.id == arrival.id }) return
    list.add(arrival)
    while (list.size > MAX_ARRIVALS) list.removeAt(0)
    p.edit().putString(KEY_RECEIVED, receivedJson(list)).commit()
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
        if (at >= 0L) list.add(ArrivalMessage(id, who, device, at))
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
        JSONObject().put("id", a.id).put("who", a.who).put("device", a.device).put("at", a.at)
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
