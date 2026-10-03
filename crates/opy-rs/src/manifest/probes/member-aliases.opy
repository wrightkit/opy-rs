globalvar g
rule "r":
    @Event eachPlayer
    @Condition eventPlayer.getCurrentHero() != null
    @Condition eventPlayer.hasStatusEffect(Status.BURNING) == false
    eventPlayer.setCamera(vect(0, 0, 0), vect(1, 1, 1), blendSpeed=0.5)
    eventPlayer.startDoT(damager=null, duration=2, damagePerSecond=3)
    eventPlayer.stopAllDoT()
    eventPlayer.stopAllHoT()
    disableInspector()
