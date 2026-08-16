globalvar score = 0
playervar hasStarted
subroutine showStatus

def showStatus():
    print("Score: {}".format(score))

rule "player starts":
    @Event eachPlayer
    @Condition eventPlayer.hasSpawned()
    eventPlayer.hasStarted = true
    showStatus()
