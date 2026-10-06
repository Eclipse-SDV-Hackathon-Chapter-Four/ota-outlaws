# Overall Architecture


# Can Messages
|Field Name|Datatype|Range|
|---|---|---|
|CellTempMax|0-7|`uint8`|0...255|
|CellTempMin|8-15|`uint8`|0...255|
|CellTempAvg|16-23|`uint8`|0...255|
|Quality|24-25|`enum`|0...2|
|Counter|26-34|`uint8`|0...255|

# Quality Enum
UNDEFINED = 0
OK = 1
INVALID = 2