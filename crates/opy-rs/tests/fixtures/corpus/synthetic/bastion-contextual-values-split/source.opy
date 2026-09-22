globalvar index
globalvar values
playervar savedEffect

rule "Apply hero selection context":
    @Event eachPlayer
    @Condition eventPlayer.isInSpawnRoom() == true
    @Condition eventPlayer.isMoving() == false
    @Condition index == null
    index = false
    values = [null, null]
    values[true] = null
    eventPlayer.savedEffect = null
    eventPlayer.setStatusEffect(null, Status.INVINCIBLE, false)
    createEffect(null, Effect.RING, Color.RED, vect(false, true, false), false, EffectReeval.VISIBILITY)
    wait(false)
    for index in range(false, true, false):
        pass
