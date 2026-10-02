rule "stopHoT rejects the old keyword":
    stopHoT(healingOverTimeId=getLastHealingOverTimeId())
