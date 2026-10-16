globalvar g

rule "member spellings the reference does not accept":
    @Event eachPlayer
    g = Hero.SOLDIER_76
    g = Map.ROUTE_66
    g = Team.3
    g = Clip.DO_NOT_CLIP
