#!optimizeForSize
globalvar marker

rule "wait optimization":
    @Event global
    marker = 1
    wait()
    wait(0.016)
    wait(1)
    wait(1, Wait.ABORT_WHEN_FALSE)
    marker = 2
