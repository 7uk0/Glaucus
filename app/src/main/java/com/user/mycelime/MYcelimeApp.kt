package com.user.mycelime

import android.app.Application
import com.user.mycelime.data.AppDatabase
import com.user.mycelime.data.NurseryRepository

class MYcelimeApp : Application() {
    val database by lazy { AppDatabase.getInstance(this) }
    val repository by lazy { NurseryRepository(database) }
}
