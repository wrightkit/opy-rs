globalvar total = 0

subroutine resetScore

enum Direction:
    NORTH
    SOUTH

macro doubleIt(value):
    value + value

def finish():
    total = 0
