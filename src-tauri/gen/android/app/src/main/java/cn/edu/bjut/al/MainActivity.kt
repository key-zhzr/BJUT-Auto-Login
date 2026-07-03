package cn.edu.bjut.al

import android.os.Bundle
import android.content.Context
import android.net.wifi.WifiManager
import androidx.activity.enableEdgeToEdge

class MainActivity : TauriActivity() {
  override fun onCreate(savedInstanceState: Bundle?) {
    enableEdgeToEdge()
    super.onCreate(savedInstanceState)
  }
}

