package com.user.mycelime.data

import android.content.Context
import androidx.room.Database
import androidx.room.Room
import androidx.room.RoomDatabase
import com.user.mycelime.data.dao.CreatureDao
import com.user.mycelime.data.dao.EggDao
import com.user.mycelime.data.dao.ExpeditionDao
import com.user.mycelime.data.dao.NurseryStateDao
import com.user.mycelime.data.entity.Creature
import com.user.mycelime.data.entity.Egg
import com.user.mycelime.data.entity.Expedition
import com.user.mycelime.data.entity.NurseryState

@Database(
    entities = [Creature::class, Egg::class, Expedition::class, NurseryState::class],
    version = 1,
    exportSchema = false,
)
abstract class AppDatabase : RoomDatabase() {
    abstract fun creatureDao(): CreatureDao
    abstract fun eggDao(): EggDao
    abstract fun expeditionDao(): ExpeditionDao
    abstract fun nurseryStateDao(): NurseryStateDao

    companion object {
        @Volatile private var instance: AppDatabase? = null

        fun getInstance(context: Context): AppDatabase =
            instance ?: synchronized(this) {
                instance ?: Room.databaseBuilder(
                    context.applicationContext,
                    AppDatabase::class.java,
                    "mycelime.db",
                ).build().also { instance = it }
            }
    }
}
