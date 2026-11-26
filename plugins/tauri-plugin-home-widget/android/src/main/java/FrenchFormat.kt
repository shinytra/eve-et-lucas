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

  /** Le jour existe (mois de 1 à 12 ; le 29 février seulement les années bissextiles). */
  fun isValidDay(year: Int, month: Int, day: Int): Boolean {
    if (year !in 1900..2200 || month !in 1..12 || day < 1) return false
    val cal = Calendar.getInstance()
    cal.clear()
    cal.set(year, month - 1, 1)
    return day <= cal.getActualMaximum(Calendar.DAY_OF_MONTH)
  }

  /** Nombre de jours entre le jour de `now` et le jour donné (mois de 1 à 12). */
  fun daysUntil(year: Int, month: Int, day: Int, now: Long): Int {
    val cal = Calendar.getInstance()
    cal.clear()
    cal.set(year, month - 1, day, 12, 0, 0)
    return dayOffset(cal.timeInMillis, now)
  }

  /** Prochain minuit local après `now`. */
  fun nextMidnight(now: Long): Long {
    val cal = calendar(now)
    cal.set(Calendar.HOUR_OF_DAY, 0)
    cal.set(Calendar.MINUTE, 0)
    cal.set(Calendar.SECOND, 0)
    cal.set(Calendar.MILLISECOND, 0)
    cal.add(Calendar.DAY_OF_MONTH, 1)
    return cal.timeInMillis
  }

  /**
   * « 🎂 Dans 12 jours : Anniversaire d'Eve (25 ans) », « 💞 Demain : Nous
   * (6 ans) », « 🎉 Aujourd'hui : Noël ». Mêmes mots que les titres des
   * rappels (`reminder` dans `couple_core::dates`) : changer les deux ensemble.
   * Le compte à rebours vient avant le titre pour rester visible si le titre
   * est coupé. `null` si la date est passée (appli pas ouverte depuis).
   */
  fun countdown(emoji: String, title: String, years: Int?, daysLeft: Int): String? {
    val whenText = when {
      daysLeft < 0 -> return null
      daysLeft == 0 -> "Aujourd'hui"
      daysLeft == 1 -> "Demain"
      else -> "Dans $daysLeft jours"
    }
    val age = when {
      years == null || years <= 0 -> ""
      years == 1 -> " (1 an)"
      else -> " ($years ans)"
    }
    return "$emoji $whenText : $title$age"
  }
}
