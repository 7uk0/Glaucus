package com.user.mycelime.data.dao

import androidx.room.*
import com.user.mycelime.data.entity.Creature
import kotlinx.coroutines.flow.Flow

@Dao
interface CreatureDao {
    @Query("SELECT * FROM creatures")
    fun getAllCreatures(): Flow<List<Creature>>

    @Query("SELECT * FROM creatures WHERE isAlive = 1")
    fun getLivingCreatures(): Flow<List<Creature>>

    @Insert(onConflict = OnConflictStrategy.REPLACE)
    suspend fun insert(creature: Creature)

    @Update
    suspend fun update(creature: Creature)

    @Delete
    suspend fun delete(creature: Creature)
}
