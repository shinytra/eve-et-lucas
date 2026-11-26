package fr.eveetlucas.homewidget

import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.Context

/**
 * Le widget. Android l'appelle quand il est posé, après un redémarrage, une
 * mise à jour de l'appli et à minuit (alarme posée par [HomeWidgetUpdater]) ;
 * le reste du temps, c'est l'appli (`update`) ou une case cochée qui le
 * redessine.
 */
class HomeWidgetProvider : AppWidgetProvider() {
  override fun onUpdate(context: Context, appWidgetManager: AppWidgetManager, appWidgetIds: IntArray) {
    HomeWidgetUpdater.update(context, appWidgetManager, appWidgetIds)
  }
}
