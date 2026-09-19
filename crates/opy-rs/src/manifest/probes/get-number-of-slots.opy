globalvar value

rule "number of slots":
    @Event global
    value = getNumberOfSlots(Team.1)
