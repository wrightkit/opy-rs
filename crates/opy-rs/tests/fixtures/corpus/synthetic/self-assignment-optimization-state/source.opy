globalvar g
globalvar A
playervar foo
playervar B

macro self_inc():
    g = g + 1

macro idx():
    1

macro arr_inc():
    A[idx()] = A[idx()] + 1

macro pvar_inc():
    eventPlayer.foo = eventPlayer.foo + 1

macro arr_pvar_inc():
    eventPlayer.B[idx()] = eventPlayer.B[idx()] + 1

macro nested_inc():
    A[idx()][idx()] = A[idx()][idx()] + 1

macro all_ops():
    g = g + 1
    g = g - 1
    g = g * 2
    g = g / 2
    g = g % 2
    g = g ** 2
    A[0] = A[0] + 1
    A[1] = A[1] - 1
    A[1] = A[1] * 2
    A[1] = A[1] / 2
    A[1] = A[1] % 2
    A[1] = A[1] ** 2
    eventPlayer.foo = eventPlayer.foo + 1
    eventPlayer.foo = eventPlayer.foo - 1
    eventPlayer.foo = eventPlayer.foo * 2
    eventPlayer.foo = eventPlayer.foo / 2
    eventPlayer.foo = eventPlayer.foo % 2
    eventPlayer.foo = eventPlayer.foo ** 2
    eventPlayer.B[1] = eventPlayer.B[1] + 1
    A[1][2] = A[1][2] + 1
    g = g + 0
    g = g * 1
    g = g
    g = 1 + g
    A[1] = A[idx()] + 1
    A[idx()] = A[idx()] + 1

rule "optimized":
    @Event eachPlayer
    all_ops()
    self_inc()
    arr_inc()
    pvar_inc()
    arr_pvar_inc()
    nested_inc()

#!disableOptimizations
rule "disabled":
    @Event eachPlayer
    all_ops()
    self_inc()
    arr_inc()
    pvar_inc()
    arr_pvar_inc()
    nested_inc()

#!enableOptimizations
#!optimizeStrict
rule "strict":
    @Event eachPlayer
    all_ops()

#!disableOptimizeStrict
#!optimizeForSize
rule "size":
    @Event eachPlayer
    all_ops()
