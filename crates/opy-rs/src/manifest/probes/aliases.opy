globalvar g
rule "r":
    @Event global
    stopChasingVariable(g)
    stopChasing(g)
    destroyAllInWorldText()
    g = getLastAssistID()
    g = getLastDoT()
    g = getNumberOfDoTIds()
    g = getNumberOfHoTIds()
    g = rgba(255, 0, 0, 128)
    g = rgba(255, 0, 0)
    g = hsla(120, 0.5, 0.5, 200)
    g = hsla(0, 1, 0.5)
    g = teamHasHero(Hero.ANA, Team.ALL)
    stopDoT(getLastDoT())
