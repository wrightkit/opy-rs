globalvar phase
globalvar hits

rule "condition transition":
    @Event global
    @Condition (phase == false and getMatchTime() > 0) == true
    hits += 1
    phase = true
    wait(0.016)
