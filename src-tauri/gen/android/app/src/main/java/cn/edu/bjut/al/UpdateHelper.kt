package cn.edu.bjut.al

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.Settings
import androidx.core.content.FileProvider
import java.io.File

class UpdateHelper {
    companion object {
        private const val UPDATE_PREFERENCES = "bjut_al_update"
        private const val PENDING_APK_PATH = "pending_apk_path"

        private fun setPendingApkPath(context: Context, path: String?) {
            val editor = context.applicationContext
                .getSharedPreferences(UPDATE_PREFERENCES, Context.MODE_PRIVATE)
                .edit()
            if (path == null) editor.remove(PENDING_APK_PATH) else editor.putString(PENDING_APK_PATH, path)
            editor.apply()
        }

        private fun getPendingApkPath(context: Context): String? =
            context.applicationContext
                .getSharedPreferences(UPDATE_PREFERENCES, Context.MODE_PRIVATE)
                .getString(PENDING_APK_PATH, null)

        @JvmStatic
        fun installApk(context: Context, path: String): Boolean {
            return try {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O && !context.packageManager.canRequestPackageInstalls()) {
                    setPendingApkPath(context, path)
                    val settingsIntent = Intent(
                        Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES,
                        Uri.parse("package:${context.packageName}")
                    ).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                    context.startActivity(settingsIntent)
                    return true
                }

                val apk = File(path)
                if (!apk.isFile) return false
                val uri = FileProvider.getUriForFile(
                    context,
                    "${context.packageName}.fileprovider",
                    apk
                )
                val installIntent = Intent(Intent.ACTION_VIEW).apply {
                    setDataAndType(uri, "application/vnd.android.package-archive")
                    addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                    addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
                }
                context.startActivity(installIntent)
                setPendingApkPath(context, null)
                true
            } catch (error: Exception) {
                error.printStackTrace()
                false
            }
        }

        @JvmStatic
        fun resumePendingInstall(context: Context) {
            val path = getPendingApkPath(context) ?: return
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O || context.packageManager.canRequestPackageInstalls()) {
                installApk(context, path)
            }
        }
    }
}
