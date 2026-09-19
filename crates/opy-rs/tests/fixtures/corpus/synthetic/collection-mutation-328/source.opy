globalvar a
globalvar i
playervar p

rule "global 1d":
    @Event global
    del a[0]

rule "player 1d":
    @Event eachPlayer
    del eventPlayer.p[0]

rule "global 2d":
    @Event global
    del a[i][0]

rule "global 3d":
    @Event global
    del a[i][0][0]

rule "literal 3d":
    @Event global
    del a[0][1][2]

rule "global 4d":
    @Event global
    del a[0][1][2][3]

rule "random leaf 3d":
    @Event global
    del a[0][0][random.randint(0, 1)]
