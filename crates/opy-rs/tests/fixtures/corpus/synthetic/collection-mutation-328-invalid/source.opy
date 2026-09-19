globalvar a

rule "five-index delete":
    @Event global
    del a[0][1][2][3][4]
