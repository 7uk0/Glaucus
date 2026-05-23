package com.user.mycelime.data

import com.user.mycelime.data.entity.Creature
import com.user.mycelime.data.entity.Egg
import com.user.mycelime.data.entity.Expedition
import com.user.mycelime.data.entity.NurseryState

class NurseryRepository(private val db: AppDatabase) {
    val eggs = db.eggDao().getUnhatchedEggs()
    val creatures = db.creatureDao().getLivingCreatures()
    val activeExpeditions = db.expeditionDao().getActiveExpeditions()
    val nurseryState = db.nurseryStateDao().getNurseryState()

    suspend fun insertEgg(egg: Egg) = db.eggDao().insert(egg)
    suspend fun updateEgg(egg: Egg) = db.eggDao().update(egg)

    suspend fun insertCreature(creature: Creature) = db.creatureDao().insert(creature)
    suspend fun updateCreature(creature: Creature) = db.creatureDao().update(creature)

    suspend fun insertExpedition(expedition: Expedition) = db.expeditionDao().insert(expedition)
    suspend fun updateExpedition(expedition: Expedition) = db.expeditionDao().update(expedition)

    suspend fun upsertNurseryState(state: NurseryState) = db.nurseryStateDao().upsert(state)
}
