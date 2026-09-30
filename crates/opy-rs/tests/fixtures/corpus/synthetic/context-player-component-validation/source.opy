globalvar g

rule "host and local components":
    @Event eachPlayer
    g = hostPlayer.y
    g = localPlayer.z

rule "death components":
    @Event playerDied
    g = attacker.x
    g = victim.z
