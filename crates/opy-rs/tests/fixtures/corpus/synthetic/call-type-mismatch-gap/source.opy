globalvar g

rule "type-mismatched arguments the reference writes unchecked":
    @Event eachPlayer
    g = getLivingPlayers(3)
    g = getLivingPlayers("a")
    g = len(3)
