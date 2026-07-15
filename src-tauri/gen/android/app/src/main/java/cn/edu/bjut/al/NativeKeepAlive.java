package cn.edu.bjut.al;

/** Stable JNI boundary used when Android restarts the foreground service without an Activity. */
public final class NativeKeepAlive {
    static {
        System.loadLibrary("tauri_app_lib");
    }

    private NativeKeepAlive() {}

    public static native String runHeadlessCheck(
        String configJson,
        String networkInfoJson,
        String reason
    );
}
