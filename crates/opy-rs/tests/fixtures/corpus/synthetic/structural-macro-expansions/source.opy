macro CONST_ARR = [3, 1, 2]
globalvar a
globalvar b
globalvar c
globalvar d
globalvar e
globalvar f
globalvar g
globalvar h
globalvar i
globalvar j
globalvar k

rule "macro expansions":
    @Event global
    a = arrayToString(getAllPlayers(), 1)
    b = arrayToString(getAllPlayers(), 2)
    c = arrayToString(getAllPlayers(), 3)
    d = arrayToString(getAllPlayers(), 4)
    e = arrayToString(getAllPlayers(), 5)
    f = arrayToString(getAllPlayers(), 6)
    g = arrayToString(getAllPlayers(), 7)
    h = arrayToString(getAllPlayers(), 8)
    i = arrayToString(getAllPlayers(), 9)
    j = arrayToString(getAllPlayers())
    k = arrayToString([1, 2, 3], 2)
    i = [3, 1, 2].reverse()
    j = sorted([3, 1, 2], key=lambda el, ix: -ix)
    k = sorted(CONST_ARR, key=lambda el, ix: ix * -1)
    a = sorted([3, 1, 2], key=lambda x: -x)
    b = sorted(getAllPlayers(), key=lambda el, ix: -ix)
    c = getAllPlayers().reverse()
    d = sorted([1, [2, 3]], key=lambda el, ix: -ix)
    e = sorted([getTotalTimeElapsed(), 2], key=lambda el, ix: -ix)
    f = CONST_ARR.reverse()
    debug(getAllPlayers())
