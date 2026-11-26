package fr.eveetlucas.homewidget

import android.app.AlarmManager
import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.view.View
import android.widget.RemoteViews

/** Dessine le widget à partir des données enregistrées dans [WidgetStore]. */
object HomeWidgetUpdater {

  /** Redessine tous les widgets posés sur l'écran d'accueil. */
  fun refreshAll(context: Context) {
    val manager = AppWidgetManager.getInstance(context) ?: return
    val ids = manager.getAppWidgetIds(ComponentName(context, HomeWidgetProvider::class.java))
    if (ids == null || ids.isEmpty()) return
    update(context, manager, ids)
  }

  // `notifyAppWidgetViewDataChanged` et `setRemoteAdapter(Int, Intent)` sont
  // dépréciés depuis Android 15 au profit de `RemoteCollectionItems`
  // (Android 12+ seulement) : on les garde pour Android 7 à 11.
  @Suppress("DEPRECATION")
  fun update(context: Context, manager: AppWidgetManager, ids: IntArray) {
    val content = WidgetContent.load(context)
    val now = System.currentTimeMillis()
    for (id in ids) {
      manager.updateAppWidget(id, buildViews(context, id, content, now))
    }
    // La liste est fournie à part (HomeWidgetService) : on lui demande de se relire.
    manager.notifyAppWidgetViewDataChanged(ids, R.id.home_widget_list)
    scheduleMidnight(context, ids, now)
  }

  /**
   * Redessine le widget au prochain minuit (compte à rebours, « hier »), même
   * appli fermée. Alarme non exacte et sans réveil (`RTC`) : si le téléphone
   * dort, elle part quand l'écran se rallume, sans permission particulière.
   * Chaque dessin remplace l'alarme précédente.
   */
  private fun scheduleMidnight(context: Context, ids: IntArray, now: Long) {
    val alarms = context.getSystemService(Context.ALARM_SERVICE) as? AlarmManager ?: return
    val intent = Intent(context, HomeWidgetProvider::class.java)
    intent.action = AppWidgetManager.ACTION_APPWIDGET_UPDATE
    intent.putExtra(AppWidgetManager.EXTRA_APPWIDGET_IDS, ids)
    val pending = PendingIntent.getBroadcast(
      context,
      MIDNIGHT_REQUEST,
      intent,
      PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
    )
    // Une seconde après minuit, pour tomber sûrement sur le nouveau jour.
    alarms.setWindow(AlarmManager.RTC, FrenchFormat.nextMidnight(now) + 1_000, 60_000, pending)
  }

  /** Code de la demande de l'alarme de minuit (0 sert à ouvrir l'appli et aux cases). */
  private const val MIDNIGHT_REQUEST = 1

  @Suppress("DEPRECATION")
  private fun buildViews(
    context: Context,
    appWidgetId: Int,
    content: WidgetContent?,
    now: Long
  ): RemoteViews {
    val views = RemoteViews(context.packageName, R.layout.home_widget)

    // Toucher le titre ouvre l'appli.
    val launch = context.packageManager.getLaunchIntentForPackage(context.packageName)
    if (launch != null) {
      val open = PendingIntent.getActivity(
        context,
        0,
        launch,
        PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
      )
      views.setOnClickPendingIntent(R.id.home_widget_title, open)
    }

    val arrival = content?.lastArrival
    if (arrival != null) {
      views.setTextViewText(
        R.id.home_widget_arrival,
        FrenchFormat.arrival(arrival.who, arrival.at, now)
      )
      views.setViewVisibility(R.id.home_widget_arrival, View.VISIBLE)
    } else {
      views.setViewVisibility(R.id.home_widget_arrival, View.GONE)
    }

    // Première date pas encore passée (l'appli a pu rester fermée depuis l'envoi).
    val dateText = content?.nextDates.orEmpty().firstNotNullOfOrNull {
      FrenchFormat.countdown(
        it.emoji,
        it.title,
        it.years,
        FrenchFormat.daysUntil(it.year, it.month, it.day, now)
      )
    }
    if (dateText != null) {
      views.setTextViewText(R.id.home_widget_date, dateText)
      views.setViewVisibility(R.id.home_widget_date, View.VISIBLE)
    } else {
      views.setViewVisibility(R.id.home_widget_date, View.GONE)
    }

    // Liste des tâches. Une donnée propre à chaque widget dans l'intent, pour
    // qu'Android ne confonde pas les listes de deux widgets.
    val service = Intent(context, HomeWidgetService::class.java)
    service.putExtra(AppWidgetManager.EXTRA_APPWIDGET_ID, appWidgetId)
    service.data = Uri.parse(service.toUri(Intent.URI_INTENT_SCHEME))
    views.setRemoteAdapter(R.id.home_widget_list, service)
    views.setEmptyView(R.id.home_widget_list, R.id.home_widget_empty)

    // Modèle commun aux cases à cocher, complété par chaque ligne
    // (setOnClickFillInIntent) : il doit être MUTABLE sur Android 12+, sinon
    // l'identifiant de la tâche n'est pas ajouté.
    val done = Intent(context, WidgetActionReceiver::class.java)
    done.action = WidgetActionReceiver.ACTION_DONE
    val mutable = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) PendingIntent.FLAG_MUTABLE else 0
    val template = PendingIntent.getBroadcast(
      context,
      0,
      done,
      PendingIntent.FLAG_UPDATE_CURRENT or mutable
    )
    views.setPendingIntentTemplate(R.id.home_widget_list, template)

    val emptyText = when {
      content == null -> context.getString(R.string.home_widget_stale)
      content.others > 0 -> context.getString(R.string.home_widget_hidden_only)
      else -> context.getString(R.string.home_widget_nothing)
    }
    views.setTextViewText(R.id.home_widget_empty, emptyText)

    val others = content?.others ?: 0
    if (content != null && content.tasks.isNotEmpty() && others > 0) {
      views.setTextViewText(
        R.id.home_widget_more,
        context.resources.getQuantityString(R.plurals.home_widget_more, others, others)
      )
      views.setViewVisibility(R.id.home_widget_more, View.VISIBLE)
    } else {
      views.setViewVisibility(R.id.home_widget_more, View.GONE)
    }
    return views
  }
}
