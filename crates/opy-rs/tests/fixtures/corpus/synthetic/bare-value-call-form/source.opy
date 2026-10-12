globalvar g

rule "damage context calls":
    @Event playerTookDamage
    g = attacker()
    g = victim()
    g = eventPlayer()
    g = hostPlayer()
    g = localPlayer()
    g = eventDamage()
    g = eventDirection()
    g = eventAbility()
    g = eventWasCriticalHit()
    g = eventWasEnvironment()
    g = ruleCondition()
    g = null()
    g = true()
    g = false()
    if attacker() != null:
        eventPlayer().setMoveSpeed(150)

rule "healing context calls":
    @Event playerDealtHealing
    g = healer()
    g = healee()
    g = eventHealing()
    g = eventWasHealthPack()
