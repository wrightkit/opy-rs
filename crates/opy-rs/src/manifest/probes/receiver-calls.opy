globalvar g
rule "r":
    @Event eachPlayer
    @Condition eventPlayer.isAlive()
    @Condition eventPlayer.hasSpawned()
    @Condition eventPlayer.getHealth() < 50
    @Condition eventPlayer.getCurrentWeapon() == 1
    @Condition eventPlayer.isFiringPrimaryFire()
    @Condition eventPlayer.isFiringSecondaryFire()
    eventPlayer.setMoveSpeed(100)
    eventPlayer.setWeapon(1)
    eventPlayer.setMaxHealth(200)
    eventPlayer.setHealth(200)
    eventPlayer.teleport(eventPlayer.getPosition())
    eventPlayer.setAimSpeed(150)
    eventPlayer.setGravity(100)
    eventPlayer.setDamageDealt(50)
    eventPlayer.setDamageReceived(50)
    eventPlayer.setUltCharge(100)
    g = eventPlayer.getAmmo()
    g = eventPlayer.getMaxAmmo(0)
    g = eventPlayer
    g.setMoveSpeed(50)
    [1, 2].append(3)
    g.append(4)
