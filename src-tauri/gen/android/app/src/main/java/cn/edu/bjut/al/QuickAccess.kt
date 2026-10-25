package cn.edu.bjut.al

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.os.Build
import android.service.quicksettings.TileService
import android.widget.RemoteViews

object QuickAccess {
    val actions = setOf("dashboard", "accounts", "billing", "check", "login")
    fun intent(context: Context, action: String) = Intent(context, MainActivity::class.java)
        .setAction("cn.edu.bjut.al.QUICK_ACTION")
        .putExtra("quick_action", action)
        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
    fun pending(context: Context, action: String) = PendingIntent.getActivity(context, actions.sorted().indexOf(action), intent(context, action), PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    fun update(context: Context) {
        val manager = AppWidgetManager.getInstance(context)
        val ids = manager.getAppWidgetIds(ComponentName(context, NetworkWidget::class.java))
        if (ids.isEmpty()) return
        val views = RemoteViews(context.packageName, R.layout.network_widget)
        views.setTextViewText(R.id.widget_status, context.getSharedPreferences("service_state", Context.MODE_PRIVATE).getString("widget_status", "打开查看网络状态"))
        views.setOnClickPendingIntent(R.id.widget_status, pending(context, "dashboard"))
        views.setOnClickPendingIntent(R.id.widget_check, pending(context, "check"))
        views.setOnClickPendingIntent(R.id.widget_billing, pending(context, "billing"))
        manager.updateAppWidget(ids, views)
    }
}
class NetworkWidget : AppWidgetProvider() {
    override fun onUpdate(context: Context, manager: AppWidgetManager, ids: IntArray) { QuickAccess.update(context) }
}
class NetworkTile : TileService() {
    override fun onStartListening() { super.onStartListening(); qsTile?.apply { label="BJUT-AL"; if (Build.VERSION.SDK_INT >= 29) subtitle="检查网络"; updateTile() } }
    override fun onClick() {
        super.onClick()
        if (Build.VERSION.SDK_INT >= 34) startActivityAndCollapse(QuickAccess.pending(this,"check"))
        else { @Suppress("DEPRECATION") startActivityAndCollapse(QuickAccess.intent(this,"check")) }
    }
}
