package fr.eveetlucas.sharetarget

import android.content.Context
import android.content.SharedPreferences
import android.util.Log
import app.tauri.plugin.JSObject
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID

internal const val TAG = "EveEtLucasPartage"

/** Texte reçu par le menu Partager, en attente d'être traité par l'appli. */
data class SharedText(val id: String, val text: String, val subject: String, val at: Long) {
  fun toJson(): JSONObject {
    val json = JSONObject()
    json.put("id", id)
    json.put("text", text)
    json.put("subject", subject)
    json.put("at", at)
    return json
  }

  /** Même contenu, au format attendu par `Plugin.trigger`. */
  fun toJSObject(): JSObject {
    val js = JSObject()
    js.put("id", id)
    js.put("text", text)
    js.put("subject", subject)
    js.put("at", at)
    return js
  }

  companion object {
    /** Une entrée illisible renvoie `null` (elle est ignorée, jamais bloquante). */
    fun fromJson(json: JSONObject?): SharedText? {
      if (json == null) return null
      val id = json.opt("id") as? String ?: return null
      val text = json.opt("text") as? String ?: return null
      val subject = json.opt("subject") as? String ?: ""
      val at = json.opt("at") as? Number ?: return null
      return SharedText(id, text, subject, at.toLong())
    }
  }
}

/**
 * Partages reçus et pas encore confirmés par l'appli. Chaque accès est
 * synchronisé et écrit avec `commit()` pour ne rien perdre si Android arrête
 * le processus juste après.
 */
object ShareStore {
  private const val PREFS = "fr.eveetlucas.sharetarget"
  private const val KEY_SHARES = "shares"

  /** Au-delà, les partages les plus anciens sont oubliés. */
  private const val MAX_SHARES = 50

  /** Le cœur Rust refuse de toute façon un texte plus long. */
  const val MAX_TEXT = 2000

  private fun prefs(context: Context): SharedPreferences =
    context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

  @Synchronized
  fun add(context: Context, text: String, subject: String, at: Long): SharedText {
    val share = SharedText(
      UUID.randomUUID().toString(),
      text.take(MAX_TEXT),
      subject.take(MAX_TEXT),
      at,
    )
    val p = prefs(context)
    val shares = read(p)
    shares.add(share)
    while (shares.size > MAX_SHARES) shares.removeAt(0)
    if (!p.edit().putString(KEY_SHARES, toJsonArray(shares).toString()).commit()) {
      Log.w(TAG, "Partage non enregistré")
    }
    return share
  }

  /** Renvoie les partages enregistrés, sans les effacer. */
  @Synchronized
  fun peek(context: Context): List<SharedText> = read(prefs(context))

  /** Oublie ces partages, une fois traités par l'appli. */
  @Synchronized
  fun ack(context: Context, ids: Collection<String>) {
    val forget = ids.toSet()
    val p = prefs(context)
    val rest = read(p).filter { it.id !in forget }
    p.edit().putString(KEY_SHARES, toJsonArray(rest).toString()).commit()
  }

  private fun toJsonArray(shares: List<SharedText>): JSONArray {
    val json = JSONArray()
    for (s in shares) json.put(s.toJson())
    return json
  }

  private fun read(p: SharedPreferences): MutableList<SharedText> {
    val list = mutableListOf<SharedText>()
    val raw = p.getString(KEY_SHARES, null) ?: return list
    try {
      val json = JSONArray(raw)
      for (i in 0 until json.length()) {
        val share = SharedText.fromJson(json.optJSONObject(i))
        if (share != null) list.add(share)
      }
    } catch (e: Exception) {
      Log.w(TAG, "Liste des partages illisible, ignorée", e)
    }
    return list
  }
}
