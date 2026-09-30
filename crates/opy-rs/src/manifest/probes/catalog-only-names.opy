globalvar g
rule "r":
    @Event global
    createHudText(1)
    g = squareRoot(9)
    g = evaluateOnce(1)
    g = lastCreatedEntity()
    g = lastTextId()
    g = allTankHeroes()
    g = allDamageHeroes()
    g = allSupportHeroes()
    destroyAllHudText()

rule "member":
    @Event eachPlayer
    g = eventPlayer.isButtonHeld(Button.PRIMARY_FIRE)
    g = eventPlayer.x()
    g = eventPlayer.y()
    g = eventPlayer.z()
