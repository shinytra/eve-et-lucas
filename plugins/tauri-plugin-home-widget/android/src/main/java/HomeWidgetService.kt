package fr.eveetlucas.homewidget

import android.content.Context
import android.content.Intent
import android.view.View
import android.widget.RemoteViews
import android.widget.RemoteViewsService

/** Fournit au lanceur les lignes de la liste des tâches. */
class HomeWidgetService : RemoteViewsService() {
  override fun onGetViewFactory(intent: Intent): RemoteViewsService.RemoteViewsFactory =
    TaskListFactory(applicationContext)
}

private class TaskListFactory(private val context: Context) : RemoteViewsService.RemoteViewsFactory {
  private var tasks: List<WidgetTask> = emptyList()
  private var now: Long = System.currentTimeMillis()

  override fun onCreate() {}

  /** Appelé (hors du fil principal) après `notifyAppWidgetViewDataChanged`. */
  override fun onDataSetChanged() {
    tasks = WidgetContent.load(context)?.tasks ?: emptyList()
    now = System.currentTimeMillis()
  }

  override fun onDestroy() {
    tasks = emptyList()
  }

  override fun getCount(): Int = tasks.size

  override fun getViewAt(position: Int): RemoteViews {
    val views = RemoteViews(context.packageName, R.layout.home_widget_item)
    // La liste a pu changer entre-temps : ligne vide plutôt qu'un plantage.
    val task = tasks.getOrNull(position) ?: return views

    views.setTextViewText(R.id.home_widget_item_title, task.title)

    val parts = mutableListOf<String>()
    if (task.assignee.isNotEmpty()) {
      parts.add(context.getString(R.string.home_widget_for, task.assignee))
    }
    val due = task.dueAt
    if (due != null) parts.add(FrenchFormat.due(due, now))
    // Pas seulement la couleur : « en retard » est aussi écrit.
    if (task.late) parts.add(context.getString(R.string.home_widget_late))
    if (parts.isEmpty()) {
      views.setViewVisibility(R.id.home_widget_item_subtitle, View.GONE)
    } else {
      views.setTextViewText(R.id.home_widget_item_subtitle, parts.joinToString(" · "))
      views.setViewVisibility(R.id.home_widget_item_subtitle, View.VISIBLE)
      val color = if (task.late) R.color.home_widget_late else R.color.home_widget_text_secondary
      views.setTextColor(R.id.home_widget_item_subtitle, context.getColor(color))
    }

    val fillIn = Intent()
    fillIn.putExtra(WidgetActionReceiver.EXTRA_TASK_ID, task.id)
    views.setOnClickFillInIntent(R.id.home_widget_item_check, fillIn)
    views.setContentDescription(
      R.id.home_widget_item_check,
      context.getString(R.string.home_widget_check, task.title)
    )
    return views
  }

  override fun getLoadingView(): RemoteViews? = null

  override fun getViewTypeCount(): Int = 1

  override fun getItemId(position: Int): Long =
    tasks.getOrNull(position)?.id?.hashCode()?.toLong() ?: position.toLong()

  override fun hasStableIds(): Boolean = true
}
