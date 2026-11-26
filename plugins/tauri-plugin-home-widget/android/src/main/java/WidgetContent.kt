package fr.eveetlucas.homewidget

import android.content.Context
import android.util.Log
import org.json.JSONArray
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

/** Date importante à venir (jour du calendrier, sans heure ni fuseau). */
data class NextDate(
  val title: String,
  val emoji: String,
  val year: Int,
  /** 1 à 12. */
  val month: Int,
  val day: Int,
  /** Années fêtées ce jour-là, ou `null`. */
  val years: Int?
)

/**
 * Ce que le widget affiche : les données reçues de l'appli, sans les tâches
 * déjà cochées sur le widget.
 */
data class WidgetContent(
  val tasks: List<WidgetTask>,
  /** Nombre de tâches à faire au total (au moins `tasks.size`). */
  val openCount: Int,
  val lastArrival: LastArrival?,
  /** Prochaines dates, la plus proche en premier (certaines peuvent être passées). */
  val nextDates: List<NextDate>
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
        WidgetContent(
          visible,
          openCount,
          parseArrival(json.optJSONObject("lastArrival")),
          parseDates(json.optJSONArray("nextDates"))
        )
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

    private fun parseDates(array: JSONArray?): List<NextDate> {
      if (array == null) return emptyList()
      return (0 until array.length()).mapNotNull { parseDate(array.optJSONObject(it)) }
    }

    /** Une date illisible ou impossible (31 avril…) est ignorée. */
    private fun parseDate(json: JSONObject?): NextDate? {
      if (json == null) return null
      val title = json.optStringOrNull("title") ?: return null
      val day = json.optJSONObject("day") ?: return null
      val y = (day.opt("year") as? Number)?.toInt() ?: return null
      val m = (day.opt("month") as? Number)?.toInt() ?: return null
      val d = (day.opt("day") as? Number)?.toInt() ?: return null
      if (!FrenchFormat.isValidDay(y, m, d)) return null
      return NextDate(
        title = title,
        emoji = json.optStringOrNull("emoji") ?: "📅",
        year = y,
        month = m,
        day = d,
        years = (json.opt("years") as? Number)?.toInt()
      )
    }
  }
}
