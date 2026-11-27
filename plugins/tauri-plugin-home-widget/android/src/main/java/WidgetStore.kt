package fr.eveetlucas.homewidget

import android.content.Context
import android.content.SharedPreferences
import android.util.Log
import app.tauri.plugin.JSObject
import org.json.JSONArray
import org.json.JSONObject
import java.util.UUID

/** Action faite sur le widget, en attente d'être appliquée par l'appli. */
data class WidgetAction(val id: String, val kind: String, val taskId: String, val at: Long) {
  fun toJson(): JSONObject {
    val json = JSONObject()
    json.put("id", id)
    json.put("kind", kind)
    json.put("taskId", taskId)
    json.put("at", at)
    return json
  }

  /** Même contenu, au format attendu par `Plugin.trigger`. */
  fun toJSObject(): JSObject {
    val js = JSObject()
    js.put("id", id)
    js.put("kind", kind)
    js.put("taskId", taskId)
    js.put("at", at)
    return js
  }

  companion object {
    const val KIND_DONE = "done"

    /** Une entrée illisible renvoie `null` (elle est ignorée, jamais bloquante). */
    fun fromJson(json: JSONObject?): WidgetAction? {
      if (json == null) return null
      val id = json.optStringOrNull("id") ?: return null
      val kind = json.optStringOrNull("kind") ?: return null
      val taskId = json.optStringOrNull("taskId") ?: return null
      val at = json.opt("at") as? Number ?: return null
      return WidgetAction(id, kind, taskId, at.toLong())
    }
  }
}

/**
 * Préférences du plugin : les données affichées (JSON reçu de l'appli) et les
 * actions faites sur le widget pas encore confirmées par l'appli.
 *
 * Partagées entre le plugin (appli ouverte) et les récepteurs du widget :
 * chaque accès est synchronisé et écrit avec `commit()` pour ne rien perdre si
 * Android arrête le processus juste après.
 */
object WidgetStore {
  private const val PREFS = "fr.eveetlucas.homewidget"
  private const val KEY_DATA = "data"
  private const val KEY_ACTIONS = "actions"
  private const val KEY_PRESENCE = "presence_received"

  /** Personnes dont on garde la présence reçue par ntfy. */
  private const val MAX_PRESENCE = 8

  /** Au-delà, les actions les plus anciennes sont oubliées. */
  private const val MAX_ACTIONS = 200

  private fun prefs(context: Context): SharedPreferences =
    context.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

  @Synchronized
  fun saveData(context: Context, json: String) {
    prefs(context).edit().putString(KEY_DATA, json).commit()
  }

  /** Données affichées, telles que reçues (peut-être illisibles), ou `null`. */
  @Synchronized
  fun data(context: Context): String? = prefs(context).getString(KEY_DATA, null)

  /**
   * Présence d'un autre reçue par ntfy, appli fermée : affichée jusqu'à ce
   * que l'appli envoie plus récent. Une plus ancienne que celle gardée est
   * ignorée.
   */
  @Synchronized
  fun savePresence(context: Context, presence: Presence) {
    val map = receivedPresence(context).associateBy { it.who.lowercase() }.toMutableMap()
    val key = presence.who.lowercase()
    val known = map[key]
    if (known != null && known.at >= presence.at) return
    map[key] = presence
    val json = JSONArray()
    for (p in map.values.sortedByDescending { it.at }.take(MAX_PRESENCE)) json.put(p.toJson())
    prefs(context).edit().putString(KEY_PRESENCE, json.toString()).commit()
  }

  @Synchronized
  fun receivedPresence(context: Context): List<Presence> {
    val raw = prefs(context).getString(KEY_PRESENCE, null) ?: return emptyList()
    return try {
      val json = JSONArray(raw)
      (0 until json.length()).mapNotNull { Presence.fromJson(json.optJSONObject(it)) }
    } catch (e: Exception) {
      Log.w(TAG, "Présences reçues illisibles, ignorées", e)
      emptyList()
    }
  }

  /**
   * Note que la tâche `taskId` a été cochée et la retire tout de suite des
   * données affichées (affichage optimiste, en attendant que l'appli
   * renvoie des données à jour).
   *
   * Renvoie l'action créée, ou `null` si cette tâche était déjà cochée et pas
   * encore confirmée (double toucher).
   */
  @Synchronized
  fun addDone(context: Context, taskId: String, at: Long): WidgetAction? {
    val p = prefs(context)
    val actions = readActions(p)
    if (actions.any { it.kind == WidgetAction.KIND_DONE && it.taskId == taskId }) return null
    val action = WidgetAction(UUID.randomUUID().toString(), WidgetAction.KIND_DONE, taskId, at)
    actions.add(action)
    while (actions.size > MAX_ACTIONS) actions.removeAt(0)

    val editor = p.edit().putString(KEY_ACTIONS, toJsonArray(actions).toString())
    val data = p.getString(KEY_DATA, null)
    if (data != null) {
      val withoutTask = removeTask(data, taskId)
      if (withoutTask != null) editor.putString(KEY_DATA, withoutTask)
    }
    editor.commit()
    return action
  }

  /** Renvoie les actions enregistrées, sans les effacer. */
  @Synchronized
  fun peekActions(context: Context): List<WidgetAction> = readActions(prefs(context))

  /** Identifiants des tâches cochées sur le widget et pas encore confirmées. */
  @Synchronized
  fun pendingDoneTaskIds(context: Context): Set<String> =
    readActions(prefs(context))
      .filter { it.kind == WidgetAction.KIND_DONE }
      .map { it.taskId }
      .toSet()

  /** Oublie ces actions, une fois appliquées par l'appli. */
  @Synchronized
  fun ackActions(context: Context, ids: Collection<String>) {
    if (ids.isEmpty()) return
    val forget = ids.toHashSet()
    val rest = readActions(prefs(context)).filter { it.id !in forget }
    prefs(context).edit().putString(KEY_ACTIONS, toJsonArray(rest).toString()).commit()
  }

  private fun toJsonArray(actions: List<WidgetAction>): JSONArray {
    val json = JSONArray()
    for (a in actions) json.put(a.toJson())
    return json
  }

  /** Une liste illisible est ignorée, une entrée illisible aussi. */
  private fun readActions(p: SharedPreferences): MutableList<WidgetAction> {
    val list = mutableListOf<WidgetAction>()
    val raw = p.getString(KEY_ACTIONS, null) ?: return list
    try {
      val json = JSONArray(raw)
      for (i in 0 until json.length()) {
        val action = WidgetAction.fromJson(json.optJSONObject(i))
        if (action != null) list.add(action)
      }
    } catch (e: Exception) {
      Log.w(TAG, "Liste des actions du widget illisible, ignorée", e)
    }
    return list
  }

  /**
   * Données sans la tâche `taskId` (et avec un `openCount` diminué d'autant),
   * ou `null` si elles sont illisibles ou ne contiennent pas cette tâche.
   */
  private fun removeTask(data: String, taskId: String): String? {
    return try {
      val json = JSONObject(data)
      val tasks = json.optJSONArray("tasks") ?: return null
      val kept = JSONArray()
      var removed = 0
      for (i in 0 until tasks.length()) {
        val task = tasks.opt(i)
        if (task is JSONObject && task.optStringOrNull("id") == taskId) {
          removed++
        } else {
          kept.put(task)
        }
      }
      if (removed == 0) return null
      json.put("tasks", kept)
      val open = json.opt("openCount") as? Number
      if (open != null) json.put("openCount", maxOf(0, open.toInt() - removed))
      json.toString()
    } catch (e: Exception) {
      Log.w(TAG, "Données du widget illisibles, tâche cochée non retirée", e)
      null
    }
  }
}

/**
 * Texte de `key`, ou `null` s'il manque, est `null` en JSON ou n'est pas un
 * texte (`optString` renverrait "null" ou convertirait un nombre).
 */
internal fun JSONObject.optStringOrNull(key: String): String? = opt(key) as? String

internal const val TAG = "EveLucasHomeWidget"
