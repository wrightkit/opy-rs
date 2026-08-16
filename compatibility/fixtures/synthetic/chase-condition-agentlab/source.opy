globalvar Round_Attack_Time

rule "chase and condition":
    @Condition isGameInProgress() == true
    @Condition Round_Attack_Time != 0

    chaseOverTime(Round_Attack_Time, 0, 30, ChaseTimeReeval.NONE)
