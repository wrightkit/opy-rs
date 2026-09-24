globalvar g

rule "maps":
    @Event global
    g = getCurrentMap()
    g = getCurrentMap() == Map.COLOSSEO
    g = getCurrentMap() == Map.BUSAN
    g = Map.SAMOA == getCurrentMap()
    g = getCurrentMap() != Map.ESPERANCA
    g = [Map.THRONE_OF_ANUBIS, Map.BUSAN]
