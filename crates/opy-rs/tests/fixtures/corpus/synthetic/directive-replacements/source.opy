#!optimizeForSize
#!replace0ByCapturePercentage
#!replace1ByMatchRound
#!replaceTeam1ByControlScoringTeam
#!replaceEmptyStringByEmptyArray
rule "replacement directives":
    @Event global
    print(0)
    print(1)
    print(Team.1)
    print("")
