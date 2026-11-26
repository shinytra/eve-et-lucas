package fr.eveetlucas.geofence

import android.content.Context
import android.content.SharedPreferences
import org.json.JSONArray

/** Coordonnées de la maison enregistrées. */
data class Home(val lat: Double, val lng: Double, val radiusM: Float)

/**
 * Préférences du plugin : la maison (pour la réenregistrer après un
 * redémarrage), l'option « sonner à l'arrivée » et les arrivées pas encore
 * lues par l'appli.
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

  @Synchronized
  fun addArrival(context: Context, at: Long) {
    val arrivals = readArrivals(prefs(context))
    arrivals.add(at)
    while (arrivals.size > MAX_ARRIVALS) arrivals.removeAt(0)
    val json = JSONArray()
    for (t in arrivals) json.put(t)
    prefs(context).edit().putString(KEY_ARRIVALS, json.toString()).commit()
  }

  /** Renvoie les arrivées enregistrées, sans les effacer. */
  @Synchronized
  fun peekArrivals(context: Context): List<Long> = readArrivals(prefs(context))

  /**
   * Efface les arrivées jusqu'à `upTo` inclus, une fois que l'appli les a
   * enregistrées : une arrivée survenue entre-temps est gardée.
   */
  @Synchronized
  fun ackArrivals(context: Context, upTo: Long) {
    val rest = readArrivals(prefs(context)).filter { it > upTo }
    val json = JSONArray()
    for (t in rest) json.put(t)
    prefs(context).edit().putString(KEY_ARRIVALS, json.toString()).commit()
  }

  /** Une liste illisible est ignorée, jamais bloquante. */
  private fun readArrivals(p: SharedPreferences): MutableList<Long> {
    val list = mutableListOf<Long>()
    val raw = p.getString(KEY_ARRIVALS, null) ?: return list
    try {
      val json = JSONArray(raw)
      for (i in 0 until json.length()) {
        val t = json.optLong(i, -1L)
        if (t >= 0L) list.add(t)
      }
    } catch (e: Exception) {
      android.util.Log.w(TAG, "Liste des arrivées illisible, ignorée", e)
    }
    return list
  }
}

internal const val TAG = "EveLucasGeofence"
