package fr.eveetlucas.homewidget

import java.util.Calendar

/**
 * Dates et heures en français, au format court (« 18 h 42 », « demain 9 h »,
 * « 12 nov. »). `Calendar` plutôt que `java.time`, absent avant Android 8.
 */
object FrenchFormat {
  private val MONTHS = arrayOf(
    "janv.", "févr.", "mars", "avr.", "mai", "juin",
    "juil.", "août", "sept.", "oct.", "nov.", "déc."
  )

  private fun calendar(millis: Long): Calendar {
    val cal = Calendar.getInstance()
    cal.timeInMillis = millis
    return cal
  }

  /** « 18 h 42 », ou « 18 h » à l'heure pile si `dropZeroMinutes`. */
  fun time(millis: Long, dropZeroMinutes: Boolean): String {
    val cal = calendar(millis)
    val h = cal.get(Calendar.HOUR_OF_DAY)
    val m = cal.get(Calendar.MINUTE)
    if (m == 0 && dropZeroMinutes) return "$h h"
    return if (m < 10) "$h h 0$m" else "$h h $m"
  }

  /** Nombre de jours entre le jour de `now` et celui de `millis` (0 = aujourd'hui). */
  fun dayOffset(millis: Long, now: Long): Int {
    val a = startOfDay(now)
    val b = startOfDay(millis)
    // Arrondi : un passage à l'heure d'été raccourcit le jour d'une heure.
    return Math.round((b - a) / 86_400_000.0).toInt()
  }

  private fun startOfDay(millis: Long): Long {
    val cal = calendar(millis)
    cal.set(Calendar.HOUR_OF_DAY, 0)
    cal.set(Calendar.MINUTE, 0)
    cal.set(Calendar.SECOND, 0)
    cal.set(Calendar.MILLISECOND, 0)
    return cal.timeInMillis
  }

  /** « 12 nov. », avec l'année si ce n'est pas celle de `now`. */
  private fun date(millis: Long, now: Long): String {
    val cal = calendar(millis)
    val day = cal.get(Calendar.DAY_OF_MONTH)
    val month = MONTHS[cal.get(Calendar.MONTH)]
    val year = cal.get(Calendar.YEAR)
    return if (year == calendar(now).get(Calendar.YEAR)) "$day $month" else "$day $month $year"
  }

  /** Échéance : « aujourd'hui 18 h », « demain 9 h 30 », « hier 18 h », « 12 nov. 18 h ». */
  fun due(dueAt: Long, now: Long): String {
    val time = time(dueAt, dropZeroMinutes = true)
    return when (dayOffset(dueAt, now)) {
      0 -> "aujourd'hui $time"
      1 -> "demain $time"
      -1 -> "hier $time"
      else -> "${date(dueAt, now)} $time"
    }
  }

  /** « Eve est arrivée à 18 h 42 », « Lucas est arrivé hier à 7 h 05 »… */
  fun arrival(who: String, at: Long, now: Long): String {
    val verb = when (who) {
      "Eve" -> "est arrivée"
      "Lucas" -> "est arrivé"
      else -> "est arrivé(e)"
    }
    val time = time(at, dropZeroMinutes = false)
    val day = when (dayOffset(at, now)) {
      0 -> "à $time"
      -1 -> "hier à $time"
      else -> "le ${date(at, now)} à $time"
    }
    return "$who $verb $day"
  }
}
