globalvar time_reeval
globalvar rate_reeval

rule "chase reevaluation enums":
    @Event global
    time_reeval = ChaseTimeReeval.NONE
    time_reeval = ChaseTimeReeval.DESTINATION_AND_DURATION
    rate_reeval = ChaseRateReeval.NONE
    rate_reeval = ChaseRateReeval.DESTINATION_AND_RATE
