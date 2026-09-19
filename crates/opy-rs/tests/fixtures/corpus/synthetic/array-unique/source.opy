globalvar result
playervar eventTempIndex

rule "unique values":
    @Event global
    result = [1, 2, 1, 3, 2].unique()

rule "Bastion candidate pool":
    @Event eachPlayer
    eventPlayer.eventTempIndex = eventPlayer.eventTempIndex.unique()
