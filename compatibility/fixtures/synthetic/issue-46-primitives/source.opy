enum State:
    IDLE
    ACTIVE

globalvar timer = 10
globalvar total = 0
globalvar scores = [10, 20, 30]
globalvar a = [2, 4]
globalvar out = 0
globalvar hexLarge = 0x124BC
globalvar hexSmall = 0x124
playervar points = 5
playervar rank = 0
subroutine resetStats

def resetStats():
    pass

rule "update player":
    @Event eachPlayer
    @Condition eventPlayer.points > 0
    @Condition total == 0
    @Condition 20 in scores
    @Condition not (timer == 0)
    timer += 1
    timer **= 2
    total = timer * 2 + scores[1]
    scores[0] = 99
    scores[2] += 5
    eventPlayer.points = total - 10
    eventPlayer.points *= 2
    eventPlayer.points /= 2
    eventPlayer.points %= 50
    eventPlayer.A = 1
    eventPlayer.A += 2
    eventPlayer.E = eventPlayer.A
    eventPlayer.DX = eventPlayer.E
    eventPlayer.rank = State.ACTIVE
    total = "Player points: {}".format(eventPlayer.points)
    scores[1] = vect(1, 2, 3)
    A = total != 0
    B = A
    DX = scores[0] + 1
    out = a ** 2
    a **= 2
    a[0] **= 2
    resetStats()
