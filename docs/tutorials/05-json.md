# 05 · JSON

## 根就是数组

```json
[{"id":1,"name":"a"},{"id":2,"name":"b"}]
```

```sql
LOAD t FROM 'data.json';
SELECT * FROM t;
```

## 包在对象里

```json
{"code":0,"data":{"items":[{"id":1},{"id":2}]}}
```

```sql
LOAD t FROM 'resp.json' WITH (
  format='json',
  json_path='data.items'
);
```

`json_path` 是点路径，也可写成 `$.data.items`。未指定时会依次试 `data` / `items` / `results` / `records` / `rows`，否则用对象里第一个数组；都没有就把对象当**一行**。

## 行与行字段不一样

取 **key 并集**。后面行多出来的列会留下，前面没有的填 NULL。不要假设「第一行有的才是全部列」。

```sql
LOAD t FROM 'resp.json' WITH (format='json', json_path='data');
PRAGMA table_info(t);
```

## 编码

JSON 标准是 UTF-8。若接口实际给了 GBK JSON：

```sql
LOAD t FROM 'resp.json' WITH (format='json', encoding='gbk', json_path='data');
```

远程同样：`encoding='gbk'` 或响应 `charset=gbk`。

## HTTP JSON

见 [06-http-api.md](06-http-api.md)。定位符是 `https://…` 时始终走 HTTP，`format='json'` 只表示按 JSON 解码。

## 和 Excel 对上

```sql
LOAD users  FROM 'users.xlsx' WITH (format='excel', sheet='Sheet1');
LOAD flags  FROM 'https://api.example.com/flags' WITH (
  format='json',
  json_path='data'
);

SELECT u.name, f.vip
FROM users u
LEFT JOIN flags f ON u.id = f.user_id;
```

## 下一步

[HTTP：鉴权、POST、分页](06-http-api.md)
