import sqlite3
import re
import time

def convert_to_dict(string):
    # 去掉首尾的花括号和空白字符
    cleaned_string = string.strip('{} \n')

    # 使用正则表达式将字符串中的键值对分隔开
    pattern = r'(\w+\.\w+|\w+)\s*:\s*(.*?)(?=,|\n|$)'
    matches = re.findall(pattern, cleaned_string)

    # 将匹配结果转换为字典
    result_dict = {key.strip(): value.strip() for key, value in matches}

    # 将布尔值和整数字符串转换为相应的数据类型
    for key in result_dict:
        if result_dict[key].lower() == 'true':
            result_dict[key] = True
        elif result_dict[key].lower() == 'false':
            result_dict[key] = False
        elif result_dict[key].isdigit():
            result_dict[key] = int(result_dict[key])

    return result_dict


def prin_info(tag, info, count):
    print(f"分类:{tag}, 设备数{count}")
    for key in info:
        percent = "{:.2f}%".format(info[key] / count * 100)
        cur_count = info[key]
        print(f'{key}: {cur_count}/{count} ({percent})   不支持数量:{count - cur_count}')

def collect_info_func(info, config_dict):
    for key in info:
        if config_dict[key] == True:
            info[key] = info[key] + 1



# 连接到SQLite数据库
conn = sqlite3.connect('data.db')


cli_types = ["999", "h5"]

for cli_type in cli_types:
    start_time = time.perf_counter()
    cursor = conn.cursor()

    cursor.execute(f'''SELECT DATE(time) as date, COUNT(*) as player_count
    FROM upload_statistics_cli_cfg
    WHERE cli_type = '{cli_type}'
    GROUP BY DATE(time)
    ORDER BY DATE(time)''')

    rows = cursor.fetchall()
    cursor.close()

    print(f"【{cli_type}】:")
    for row in rows:
        print(f"{row[0]} : {row[1]}")

    execution_time = time.perf_counter() - start_time
    print(f"查询时间:{execution_time:.4f}秒\n\n\n")


start_time = time.perf_counter()

# 创建一个游标对象
cursor = conn.cursor()

# 执行SQL查询
cursor.execute("SELECT configuration_info, cli_type FROM upload_statistics_cli_cfg")

# 初始化统计变量
real_conf_upload_count = 0
collect_info = {
    "supports_ETC1": 0,
    "supports_ETC2": 0,
    "supports_PVRTC": 0,
    "supports_ATITC": 0,
    "supports_ASTC": 0,
    "supports_S3TC": 0,
    "supports_BGRA8888": 0,
    "supports_NPOT": 0,
    "supports_vertex_array_object": 0,
    "supports_OES_depth24": 0,
    "supports_OES_packed_depth_stencil": 0,
    "supports_discard_framebuffer": 0,
    "supports_OES_map_buffer": 0,
}

real_conf_upload_count_999 = 0
real_conf_upload_count_h5 = 0
collect_info_999 = {key: 0 for key in collect_info}
collect_info_h5 = {key: 0 for key in collect_info}
total_device_count = 0

# 逐行读取数据
while True:
    row = cursor.fetchone()
    if row is None:
        break

    configuration_info = row[0]
    cli_type = row[1]
    total_device_count = total_device_count + 1
    if configuration_info != "":
        real_conf_upload_count += 1

        config_dict = convert_to_dict(configuration_info)
        collect_info_func(collect_info, config_dict)

        if cli_type == "999":
            real_conf_upload_count_999 += 1
            collect_info_func(collect_info_999, config_dict)
        elif cli_type == "h5":
            real_conf_upload_count_h5 += 1
            collect_info_func(collect_info_h5, config_dict)


# 关闭游标和连接
cursor.close()
conn.close()

print(f"总上报设备数:{total_device_count}, 上报了客户端配置设备数:{real_conf_upload_count}")
prin_info("全部", collect_info, real_conf_upload_count)
prin_info("999", collect_info_999, real_conf_upload_count_999)
prin_info("H5", collect_info_h5, real_conf_upload_count_h5)

execution_time = time.perf_counter() - start_time
print(f"查询时间: {execution_time:.4f}秒\n\n\n")