package fr.eveetlucas.homewidget

import android.content.Context
import android.util.Log
import org.json.JSONObject

/** Une tâche à faire affichée par le widget. */
data class WidgetTask(
  val id: String,
  val title: String,
  /** "Eve", "Lucas" ou "" (personne en particulier). */
  val assignee: String,
  /** Échéance en millisecondes Unix, ou `null`. */
  val dueAt: Long?,
  val late: Boolean
)

/** Dernière arrivée à la maison. */
data class LastArrival(val who: String, val at: Long)

/**
 * Ce que le widget affiche : les données reçues de l'appli, sans les tâches
 * déjà cochées sur le widget.
 */
data class WidgetContent(
  val tasks: List<WidgetTask>,
  /** Nombre de tâches à faire au total (au moins `tasks.size`). */
  val openCount: Int,
  val lastArrival: LastArrival?
) {
  /** Tâches à faire qui ne tiennent pas dans la liste. */
  val others: Int
    get() = maxOf(0, openCount - tasks.size)

  companion object {
    /** Données enregistrées, ou `null` s'il n'y en a pas ou si elles sont illisibles. */
    fun load(context: Context): WidgetContent? =
      parse(WidgetStore.data(context), WidgetStore.pendingDoneTaskIds(context))

    /**
     * Lit le JSON produit par `couple_core::widget::WidgetData`. Une tâche
     * illisible est ignorée ; un document illisible renvoie `null`.
     */
    fun parse(raw: String?, hiddenTaskIds: Set<String>): WidgetContent? {
      if (raw == null) return null
      return try {
        val json = JSONObject(raw)
        val tasks = mutableListOf<WidgetTask>()
        val array = json.optJSONArray("tasks")
        if (array != null) {
          for (i in 0 until array.length()) {
            val task = parseTask(array.optJSONObject(i)) ?: continue
            tasks.add(task)
          }
        }
        val visible = tasks.filter { it.id !in hiddenTaskIds }
        val hidden = tasks.size - visible.size
        val declaredOpen = (json.opt("openCount") as? Number)?.toInt() ?: tasks.size
        val openCount = maxOf(visible.size, declaredOpen - hidden)
        WidgetContent(visible, openCount, parseArrival(json.optJSONObject("lastArrival")))
      } catch (e: Exception) {
        Log.w(TAG, "Données du widget illisibles", e)
        null
      }
    }

    private fun parseTask(json: JSONObject?): WidgetTask? {
      if (json == null) return null
      val id = json.optStringOrNull("id") ?: return null
      if (id.isEmpty()) return null
      return WidgetTask(
        id = id,
        title = json.optStringOrNull("title") ?: "",
        assignee = json.optStringOrNull("assignee") ?: "",
        dueAt = (json.opt("dueAt") as? Number)?.toLong(),
        late = json.opt("late") as? Boolean ?: false
      )
    }

    private fun parseArrival(json: JSONObject?): LastArrival? {
      if (json == null) return null
      val who = json.optStringOrNull("who") ?: return null
      val at = json.opt("at") as? Number ?: return null
      return LastArrival(who, at.toLong())
    }
  }
}
